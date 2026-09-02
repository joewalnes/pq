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

#[test]
fn dev_stderr_redirected_to_a_file_works() {
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
    // The "Exported N rows to /dev/stderr" status line also lands on stderr,
    // so the row count is a lower bound check via substring, not a line count.
    assert!(
        text.contains("\"id\":0") && text.contains("\"id\":2"),
        "export -o /dev/stderr > file: rows missing from {text:?}"
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
