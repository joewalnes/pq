//! Guards for `output_guard::can_stage` learning to recognise file-descriptor
//! aliases, and for the deletion of the compensation that used to live in
//! `write_output.rs` for exactly this bug.
//!
//! **The bug.** `-o /dev/stdout` resolves (via `resolve_symlinks`, following
//! macOS's `/dev/stdout -> fd/1` relative symlink) to `/dev/fd/1`. When the
//! shell has redirected stdout to a regular file, `fs::metadata("/dev/fd/1")`
//! reports exactly that — a regular file — so the old `can_stage` decided to
//! stage a sibling temp file inside the synthetic `/dev/fd` directory, which
//! devfs refuses with `ENOENT`. Measured on the pre-fix binary, serially, 5
//! runs per command: `export`, `select`, `sql`, `import`, `slice` and `merge`
//! each failed 5/5 with "cannot create a temporary file next to /dev/stdout".
//! `cat -O`/`jq -o` did not fail, because `write_output.rs` carried a
//! `names_an_open_descriptor` classifier that detected the same paths and
//! bypassed staging before ever reaching the guard — a compensation for this
//! exact bug, living one layer above where the bug actually is. This file's
//! job is to prove the guard itself now handles every affected command, and
//! that deleting the compensation didn't just move the bug back.
//!
//! These are subprocess tests (real shell redirection, real `/dev` entries)
//! because the underlying behaviour — what `/dev/stdout` resolves to, what
//! `fs::metadata` reports on it — depends on this process's own file
//! descriptors and cannot be faked in-process without disturbing the test
//! harness's own stdout/stderr.

use assert_cmd::Command;
use std::fs;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn pq() -> Command {
    assert_cmd::cargo_bin_cmd!("pq")
}

/// Absolute path to the binary under test — never a `pq` resolved by name
/// through `PATH`, which would silently substitute a stale `brew`-installed
/// build (see LESSONS.md, "A harness must assert the identity of its
/// subject").
fn pq_bin() -> PathBuf {
    let path = PathBuf::from(pq().get_program());
    assert!(
        path.is_absolute() && path.is_file(),
        "the pq binary under test is not an absolute path to a real file: {}",
        path.display()
    );
    path
}

/// POSIX single-quote a string for `/bin/sh -c`, escaping embedded single
/// quotes (`'` -> `'\''`) rather than refusing them — the `sql` argv here
/// legitimately contains single-quoted table paths inside the query text.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Run `cmd` through `/bin/sh -c`, quoting every substituted path. Never
/// build a command line by handing an unquoted variable to a shell (see
/// HAZARDS: zsh does not word-split, and neither should this).
fn run_sh(cmd: &str) -> std::process::Output {
    std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .expect("failed to spawn /bin/sh")
}

fn make_parquet(dir: &Path, name: &str, rows: usize) -> PathBuf {
    let mut body = String::new();
    for i in 0..rows {
        body.push_str(&format!("{{\"id\":{i},\"name\":\"user_{i}\"}}\n"));
    }
    let src = dir.join(format!("{name}.src.jsonl"));
    fs::write(&src, body).unwrap();
    let out = dir.join(name);
    pq().args(["import", src.to_str().unwrap(), "-o", out.to_str().unwrap()])
        .assert()
        .success();
    fs::remove_file(&src).unwrap();
    out
}

const FEW: usize = 3;

/// Whether `cmd` always writes Parquet regardless of the destination name
/// (`select`/`slice`/`merge`/`import` all rewrite/convert to Parquet; only
/// `export`/`sql` pick a text format). Determines how a test verifies the
/// captured bytes: line count for text, magic bytes + `pq count` for Parquet.
fn writes_parquet(cmd: &str) -> bool {
    matches!(cmd, "select" | "slice" | "merge" | "import")
}

/// Every argv this bug affects, keyed by name. `sql`/`export` need an
/// explicit `-f` because `/dev/stdout` has no extension to infer a format
/// from — a separate, pre-existing, unrelated requirement of those two
/// commands, not part of this bug, so it is supplied here rather than
/// treated as a failure.
fn argv_for(cmd: &str, src: &str, dest: &str) -> Vec<String> {
    match cmd {
        "export" => vec![
            "export".into(),
            src.into(),
            "-o".into(),
            dest.into(),
            "-f".into(),
            "jsonl".into(),
        ],
        "select" => vec![
            "select".into(),
            src.into(),
            "-c".into(),
            "id,name".into(),
            "-o".into(),
            dest.into(),
        ],
        "sql" => vec![
            "sql".into(),
            format!("SELECT * FROM '{src}'"),
            "-o".into(),
            dest.into(),
            "-f".into(),
            "jsonl".into(),
        ],
        "import" => vec!["import".into(), src.into(), "-o".into(), dest.into()],
        "slice" => vec![
            "slice".into(),
            src.into(),
            "--offset".into(),
            "0".into(),
            "--limit".into(),
            "2".into(),
            "-o".into(),
            dest.into(),
        ],
        "merge" => vec![
            "merge".into(),
            src.into(),
            src.into(),
            "-o".into(),
            dest.into(),
        ],
        other => panic!("unknown command in argv_for: {other}"),
    }
}

const AFFECTED_COMMANDS: &[&str] = &["export", "select", "sql", "import", "slice", "merge"];

// ---------------------------------------------------------------------------
// The class: -o /dev/stdout, redirected to a regular file, for every
// affected command.
// ---------------------------------------------------------------------------

#[test]
fn dev_stdout_redirected_to_a_file_works_for_every_affected_command() {
    let dir = TempDir::new().unwrap();
    // `import`'s src must be JSONL/CSV, not parquet; give every command its
    // own compatible source built the same way `make_parquet` builds parquet
    // (JSONL is also valid input to every other command here).
    let parquet_src = make_parquet(dir.path(), "src.parquet", FEW);
    let jsonl_src = dir.path().join("src.jsonl");
    fs::write(
        &jsonl_src,
        "{\"id\":0,\"name\":\"user_0\"}\n{\"id\":1,\"name\":\"user_1\"}\n{\"id\":2,\"name\":\"user_2\"}\n",
    )
    .unwrap();

    for &cmd in AFFECTED_COMMANDS {
        let src = if cmd == "import" {
            jsonl_src.to_str().unwrap()
        } else {
            parquet_src.to_str().unwrap()
        };
        let captured = dir.path().join(format!("{cmd}.out"));
        let _ = fs::remove_file(&captured);
        let argv = argv_for(cmd, src, "/dev/stdout");
        let quoted: Vec<String> = argv.iter().map(|a| sh_quote(a)).collect();
        let script = format!(
            "{} {} > {}",
            sh_quote(pq_bin().to_str().unwrap()),
            quoted.join(" "),
            sh_quote(captured.to_str().unwrap()),
        );
        let out = run_sh(&script);
        assert!(
            out.status.success(),
            "`{cmd} ... -o /dev/stdout > file` failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        if writes_parquet(cmd) {
            let bytes = fs::read(&captured).unwrap_or_else(|e| {
                panic!(
                    "{cmd}: could not read captured output {}: {e}",
                    captured.display()
                )
            });
            assert!(
                bytes.len() >= 4 && &bytes[..4] == b"PAR1",
                "{cmd} -o /dev/stdout > file: captured output is not Parquet: {:?}",
                &bytes[..bytes.len().min(20)]
            );
            let count_out = pq()
                .args(["count", captured.to_str().unwrap()])
                .output()
                .unwrap();
            let count_text = String::from_utf8_lossy(&count_out.stdout).to_string();
            let parsed: serde_json::Value =
                serde_json::from_str(count_text.trim()).unwrap_or_else(|e| {
                    panic!("{cmd}: `pq count` did not return JSON ({e}): {count_text:?}")
                });
            let want = match cmd {
                "merge" => FEW * 2,
                "slice" => 2, // matches the --limit 2 in argv_for
                _ => FEW,
            };
            assert_eq!(
                parsed["count"].as_u64().unwrap() as usize,
                want,
                "{cmd} -o /dev/stdout > file: wrong row count"
            );
        } else {
            let text = fs::read_to_string(&captured).unwrap_or_else(|e| {
                panic!(
                    "{cmd}: could not read captured output {}: {e}",
                    captured.display()
                )
            });
            assert_eq!(
                text.lines().count(),
                FEW,
                "{cmd} -o /dev/stdout > file: expected {FEW} rows, got {text:?}"
            );
        }
    }
}

/// Same as above, run 5 times per command, to state a count rather than a
/// single pass/fail — the P3 entry this fixes was first mis-measured as "1
/// in 5" flaky before being corrected to deterministic; this guard makes the
/// determinism claim an executable fact, not a comment.
#[test]
fn dev_stdout_redirected_to_a_file_is_deterministic_across_five_runs() {
    let dir = TempDir::new().unwrap();
    let src = make_parquet(dir.path(), "src.parquet", FEW);
    let captured = dir.path().join("out.jsonl");
    let mut failures = 0;
    const RUNS: usize = 5;
    for _ in 0..RUNS {
        let _ = fs::remove_file(&captured);
        let script = format!(
            "{} export {} -o /dev/stdout -f jsonl > {}",
            sh_quote(pq_bin().to_str().unwrap()),
            sh_quote(src.to_str().unwrap()),
            sh_quote(captured.to_str().unwrap()),
        );
        let out = run_sh(&script);
        if !out.status.success() {
            failures += 1;
        }
    }
    assert_eq!(
        failures, 0,
        "export -o /dev/stdout > file failed {failures}/{RUNS} runs"
    );
}

// ---------------------------------------------------------------------------
// /dev/stderr and bare /dev/fd/N: the same class, different names.
// ---------------------------------------------------------------------------

/// The exact bytes `make_parquet(_, _, FEW)` round-trips to as JSONL. Spelled
/// out so the assertions below can be equality, not a substring probe.
const FEW_ROWS_JSONL: &str = concat!(
    "{\"id\":0,\"name\":\"user_0\"}\n",
    "{\"id\":1,\"name\":\"user_1\"}\n",
    "{\"id\":2,\"name\":\"user_2\"}\n",
);

#[test]
fn dev_stderr_redirected_to_a_file_gets_the_rows_and_nothing_else() {
    // ------------------------------------------------------------------
    // This assertion used to be `text.contains("\"id\":0") &&
    // text.contains("\"id\":2")`, with a comment explaining that the
    // "Exported N rows to /dev/stderr" status line "also lands on stderr, so
    // the row count is a lower bound check via substring". That comment was
    // describing a bug, not a constraint. `-o /dev/stderr` makes stderr the
    // *data* stream; a status line written to the same descriptor lands in
    // the user's data file, and the two platforms corrupt different ends of
    // it:
    //
    //   macOS  `/dev/stderr -> /dev/fd/2`, and opening `/dev/fd/N` is a
    //          `dup` — shared offset — so the status line was *appended*.
    //          Measured pre-fix: 106 bytes, the 75 bytes of JSONL plus a
    //          31-byte trailing line that is not JSON. The substring check
    //          passed over the top of it.
    //   Linux  `/dev/stderr -> /proc/self/fd/2`, and opening that is a fresh
    //          open of the backing file with its own offset. The rows go in
    //          through the new description while fd 2's offset is still 0,
    //          so the status line overwrote the *first* 31 bytes — the whole
    //          of row 0 and the head of row 1. `contains("\"id\":0")` was
    //          false and CI went red here, four merges running.
    //
    // `print_status` now keeps quiet when the destination names stderr, so
    // the captured file is exactly the rows on both platforms and this can
    // be an equality check. See
    // `dev_fd_alias_open_semantics` below for the platform difference as a
    // standalone executable fact, and
    // `an_ordinary_destination_still_prints_its_status_line` for the control
    // that the suppression is narrow.
    // ------------------------------------------------------------------
    let dir = TempDir::new().unwrap();
    let src = make_parquet(dir.path(), "src.parquet", FEW);
    let captured = dir.path().join("out.jsonl");
    let script = format!(
        "{} export {} -o /dev/stderr -f jsonl 2> {} 1>/dev/null",
        sh_quote(pq_bin().to_str().unwrap()),
        sh_quote(src.to_str().unwrap()),
        sh_quote(captured.to_str().unwrap()),
    );
    let out = run_sh(&script);
    assert!(
        out.status.success(),
        "export -o /dev/stderr > file failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = fs::read_to_string(&captured).unwrap();
    assert_eq!(
        text, FEW_ROWS_JSONL,
        "export -o /dev/stderr 2> file must capture the rows and nothing else; \
         anything extra (or missing) is the status line colliding with the data"
    );
}

/// The same collision through the other name for fd 2. `/dev/stderr` is a
/// symlink on both platforms; `/dev/fd/2` is the thing it points at on macOS
/// and a differently-routed alias on Linux, so a classifier that only knew
/// the one name would leave this reachable.
#[test]
fn dev_fd_2_redirected_to_a_file_gets_the_rows_and_nothing_else() {
    let dir = TempDir::new().unwrap();
    let src = make_parquet(dir.path(), "src.parquet", FEW);
    let captured = dir.path().join("out.jsonl");
    let script = format!(
        "{} export {} -o /dev/fd/2 -f jsonl 2> {} 1>/dev/null",
        sh_quote(pq_bin().to_str().unwrap()),
        sh_quote(src.to_str().unwrap()),
        sh_quote(captured.to_str().unwrap()),
    );
    let out = run_sh(&script);
    assert!(
        out.status.success(),
        "export -o /dev/fd/2 2> file failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        fs::read_to_string(&captured).unwrap(),
        FEW_ROWS_JSONL,
        "export -o /dev/fd/2 2> file: captured bytes are not exactly the rows"
    );
}

/// Control for the suppression: it must be narrow. Without this, "never print
/// a status line at all" would satisfy every assertion above, silently
/// removing the line that eight commands print and that the golden tutorials
/// (`tests/golden/tutorials/*.md`) show in their transcripts.
#[test]
fn an_ordinary_destination_still_prints_its_status_line() {
    let dir = TempDir::new().unwrap();
    let src = make_parquet(dir.path(), "src.parquet", FEW);
    let dest = dir.path().join("out.jsonl");
    let out = pq()
        .args([
            "export",
            src.to_str().unwrap(),
            "-o",
            dest.to_str().unwrap(),
            "-f",
            "jsonl",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "export -o <file> failed");
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        stderr.contains(&format!("Exported {FEW} rows to {}", dest.display())),
        "the status line vanished for an ordinary destination: {stderr:?}"
    );
    assert_eq!(fs::read_to_string(&dest).unwrap(), FEW_ROWS_JSONL);
}

/// **Diagnostic, not a pq test.** What does this platform do when a process
/// opens `/dev/fd/N` for a descriptor it already holds? Two behaviours exist
/// and every `-o /dev/...` path in pq sits directly on the difference, so it
/// is worth holding as an executable fact rather than a claim in a comment.
///
/// The probe writes `AAAAAAAAAA` through a descriptor (offset -> 10), opens
/// that same descriptor by its `/dev/fd/N` name with `O_TRUNC`
/// (`File::create`), writes `BBB` through the new handle, then writes `CCC`
/// through the *original* handle. The two platforms disagree completely:
///
/// - **DUP** (macOS/devfs — `fd(4)`: "opening the file /dev/fd/N is
///   equivalent to duplicating file descriptor N"). Shared offset, `O_TRUNC`
///   has nothing to truncate past: `AAAAAAAAAABBBCCC`.
/// - **REOPEN** (Linux — `/dev/fd -> /proc/self/fd`, whose entries are magic
///   symlinks; opening one is a fresh open of the backing file). The file is
///   truncated to zero, `BBB` lands at offset 0, and the original handle's
///   offset is still 10, so `CCC` lands at 10 over a hole:
///   `BBB\0\0\0\0\0\0\0CCC`.
///
/// If this ever fails, the panic prints the observed bytes, which is the
/// whole point: the message is the measurement.
///
/// No `/dev/stdout`, `/dev/stderr` or fd 0/1/2 is touched — the property is
/// about `/dev/fd/N` for *any* N, and borrowing the harness's own standard
/// streams to test it would disturb the harness.
#[test]
fn dev_fd_alias_open_semantics() {
    use std::io::Write as _;
    use std::os::unix::io::AsRawFd;

    let dir = TempDir::new().unwrap();
    let probe = dir.path().join("probe");

    let mut original = fs::File::create(&probe).unwrap();
    original.write_all(b"AAAAAAAAAA").unwrap();
    original.flush().unwrap();

    let alias = format!("/dev/fd/{}", original.as_raw_fd());
    assert!(
        Path::new(&alias).exists(),
        "{alias} does not exist; this platform has no /dev/fd and the probe \
         cannot reach its subject"
    );
    {
        let mut through_alias = fs::File::create(&alias).unwrap();
        through_alias.write_all(b"BBB").unwrap();
        through_alias.flush().unwrap();
    }
    original.write_all(b"CCC").unwrap();
    original.flush().unwrap();
    drop(original);

    let observed = fs::read(&probe).unwrap();
    const DUP: &[u8] = b"AAAAAAAAAABBBCCC";
    const REOPEN: &[u8] = b"BBB\0\0\0\0\0\0\0CCC";
    let class = match observed.as_slice() {
        DUP => "DUP",
        REOPEN => "REOPEN",
        _ => "UNKNOWN",
    };
    let expected = if cfg!(target_os = "linux") {
        "REOPEN"
    } else {
        "DUP"
    };
    assert_eq!(
        class,
        expected,
        "opening /dev/fd/N behaved as {class}, not {expected}, on {}. \
         Observed bytes: {observed:?}. Everything the descriptor-alias output \
         paths assume about this platform is derived from this behaviour, so \
         a change here is a finding, not a flake.",
        std::env::consts::OS,
    );
}

#[test]
fn bare_dev_fd_1_works() {
    let dir = TempDir::new().unwrap();
    let src = make_parquet(dir.path(), "src.parquet", FEW);
    let captured = dir.path().join("out.jsonl");
    let script = format!(
        "{} export {} -o /dev/fd/1 -f jsonl > {}",
        sh_quote(pq_bin().to_str().unwrap()),
        sh_quote(src.to_str().unwrap()),
        sh_quote(captured.to_str().unwrap()),
    );
    let out = run_sh(&script);
    assert!(
        out.status.success(),
        "export -o /dev/fd/1 > file failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(fs::read_to_string(&captured).unwrap().lines().count(), FEW);
}

// ---------------------------------------------------------------------------
// Controls: everything the guard already got right must keep working.
// ---------------------------------------------------------------------------

#[test]
fn control_dev_stdout_piped_still_works() {
    let dir = TempDir::new().unwrap();
    let src = make_parquet(dir.path(), "src.parquet", FEW);
    let captured = dir.path().join("out.jsonl");
    let script = format!(
        "{} export {} -o /dev/stdout -f jsonl | cat > {}",
        sh_quote(pq_bin().to_str().unwrap()),
        sh_quote(src.to_str().unwrap()),
        sh_quote(captured.to_str().unwrap()),
    );
    let out = run_sh(&script);
    assert!(
        out.status.success(),
        "export -o /dev/stdout | cat > file failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(fs::read_to_string(&captured).unwrap().lines().count(), FEW);
}

#[test]
fn control_a_fifo_destination_still_works() {
    let dir = TempDir::new().unwrap();
    let src = make_parquet(dir.path(), "src.parquet", FEW);
    let fifo = dir.path().join("sink");
    let captured = dir.path().join("captured.jsonl");
    let status = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(status.success(), "mkfifo failed; this control cannot run");

    let script = format!(
        "cat {} > {} & {} export {} -o {} -f jsonl; rc=$?; wait; exit $rc",
        sh_quote(fifo.to_str().unwrap()),
        sh_quote(captured.to_str().unwrap()),
        sh_quote(pq_bin().to_str().unwrap()),
        sh_quote(src.to_str().unwrap()),
        sh_quote(fifo.to_str().unwrap()),
    );
    let out = run_sh(&script);
    assert!(
        out.status.success(),
        "export -o <fifo> failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        fs::symlink_metadata(&fifo).unwrap().file_type().is_fifo(),
        "the fifo was replaced by a regular file"
    );
    assert_eq!(
        fs::read_to_string(&captured).unwrap().lines().count(),
        FEW,
        "the fifo reader did not receive the rows"
    );
}

#[test]
fn control_a_normal_destination_still_stages_a_failed_write_leaves_it_intact() {
    // Proves the alias check didn't turn into a blanket "always write
    // direct": an ordinary file destination must still go through
    // stage-and-rename, which this shows by forcing a write to fail
    // part-way through (RLIMIT_FSIZE, same injector `error_display_tests.rs`
    // uses) and checking the pre-existing destination survives untouched.
    let dir = TempDir::new().unwrap();
    // Large enough that its JSONL rendering blows well past an 8-block
    // (4 KiB) file-size limit, so the failure lands mid-write.
    let src = make_parquet(dir.path(), "src.parquet", 20_000);
    let dest = dir.path().join("precious.jsonl");
    fs::write(&dest, "PRECIOUS\n").unwrap();
    let before = fs::read(&dest).unwrap();

    let script = format!(
        "trap '' XFSZ; ulimit -f 8 || exit 111; exec {} export {} -o {} -f jsonl",
        sh_quote(pq_bin().to_str().unwrap()),
        sh_quote(src.to_str().unwrap()),
        sh_quote(dest.to_str().unwrap()),
    );
    let out = run_sh(&script);
    assert_ne!(
        out.status.code(),
        Some(111),
        "the /bin/sh wrapper could not set `ulimit -f`; pq was never run"
    );
    assert!(
        !out.status.success(),
        "the command under a file-size limit SUCCEEDED; the injector never bit, \
         so this test proves nothing. stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let after = fs::read(&dest).unwrap();
    assert_eq!(
        after, before,
        "a failed write to a normal destination replaced it — staging was bypassed"
    );
    let litter: Vec<String> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.contains("pq-tmp"))
        .collect();
    assert!(litter.is_empty(), "staging litter left behind: {litter:?}");
}

#[test]
fn control_in_place_output_onto_the_input_still_works() {
    let dir = TempDir::new().unwrap();
    let src = make_parquet(dir.path(), "src.parquet", 10);
    let path = src.to_str().unwrap().to_string();
    pq().args(["select", &path, "-c", "id", "-o", &path])
        .assert()
        .success();
    let out = pq().args(["count", &path]).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let parsed: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(
        parsed["count"].as_u64().unwrap(),
        10,
        "select -o <itself> lost rows"
    );
}
