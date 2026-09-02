//! Guards for `cat -O`, `jq -o` and `cat --jq -O`: a write that fails part
//! way through must leave the destination exactly as it was.
//!
//! These three were the last file-writing paths in the workspace that did not
//! go through `pq_transform::output_guard::with_atomic_output`. They called
//! `std::fs::File::create(dest)` — `O_TRUNC` on the user's file — and only
//! then started producing bytes, so any failure after the open replaced the
//! destination with partial output.
//!
//! Measured before the fix on a deliberately full 4 MB HFS+ RAM disk
//! (`hdiutil attach -nomount ram://8192`, `diskutil erasevolume HFS+`), with a
//! 23-byte pre-existing destination and a 200,000-row input:
//!
//! ```text
//! pq cat  f      -O out.jsonl   rc=1  dest 23 -> 258,048 bytes   DESTROYED
//! pq jq   f '.'  -o out.jsonl   rc=1  dest 23 -> 258,048 bytes   DESTROYED
//! pq cat  f --jq '.' -O out.jsonl rc=1 dest 23 -> 258,048 bytes  DESTROYED
//! pq export f    -o out.jsonl   rc=1  dest 23 ->      23 bytes   INTACT (already guarded)
//! ```
//!
//! A RAM disk is not something a `cargo test` can rely on, so the failure
//! injected here is `RLIMIT_FSIZE` — writes past a byte budget fail with
//! `EFBIG`, which is the same shape as `ENOSPC`: the file is opened and
//! truncated successfully, and the failure arrives once real bytes are
//! flowing. `require_file_size_limit_enforced` proves the injector actually
//! bites before any assertion depends on it, and `run_under_file_size_limit`
//! refuses to report a result if the shell wrapper never reached pq.
//!
//! Every assertion here is on the **bytes on disk**, never on the exit code
//! alone: the pre-fix binary already exited 1 while destroying the file.

use assert_cmd::Command;
use std::fs;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::process::Output;
use tempfile::TempDir;

fn pq() -> Command {
    assert_cmd::cargo_bin_cmd!("pq")
}

/// Absolute path of the binary under test — the artefact cargo just built for
/// this test target, never a `pq` resolved by name through `PATH`. A stale
/// `/opt/homebrew/bin/pq` on `PATH` would otherwise answer these tests.
fn pq_bin() -> PathBuf {
    let path = PathBuf::from(pq().get_program());
    assert!(
        path.is_absolute() && path.is_file(),
        "the pq binary under test is not an absolute path to a real file: {}",
        path.display()
    );
    path
}

/// Rows chosen so the JSONL/CSV rendering is comfortably larger than the
/// `RLIMIT_FSIZE` budget used below, while the fixture still builds fast.
const ROWS: usize = 20_000;
/// Small fixture for the controls, where the point is "did the right bytes
/// land" rather than "is the output bigger than the limit".
const FEW: usize = 3;

fn make_parquet_rows(dir: &Path, name: &str, rows: usize) -> PathBuf {
    let mut body = String::new();
    for i in 0..rows {
        body.push_str(&format!(
            "{{\"id\":{i},\"name\":\"user_{i}\",\"note\":\"row {i}, padded so the rendering has real width\"}}\n"
        ));
    }
    let src = dir.join("make_parquet.src.jsonl");
    fs::write(&src, body).unwrap();
    let out = dir.join(name);
    pq().args(["import", src.to_str().unwrap(), "-o", out.to_str().unwrap()])
        .assert()
        .success();
    fs::remove_file(&src).unwrap();
    out
}

fn make_parquet(dir: &Path, name: &str) -> PathBuf {
    make_parquet_rows(dir, name, ROWS)
}

fn bytes(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn no_litter(dir: &Path) {
    let litter: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.contains("pq-tmp"))
        .collect();
    assert!(litter.is_empty(), "staging files left behind: {litter:?}");
}

/// Row count read back through `pq count`, an independent code path from the
/// writers under test.
fn count_rows(path: &Path) -> usize {
    let out = pq()
        .args(["count", path.to_str().unwrap()])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let parsed: serde_json::Value = serde_json::from_str(text.trim()).unwrap_or_else(|e| {
        panic!(
            "`pq count {}` did not return JSON ({e}): stdout={text:?} stderr={:?}",
            path.display(),
            String::from_utf8_lossy(&out.stderr)
        )
    });
    parsed["count"].as_u64().unwrap() as usize
}

// ---------------------------------------------------------------------------
// The failure injector: RLIMIT_FSIZE.
// ---------------------------------------------------------------------------

fn sh_quote(s: &str) -> String {
    assert!(
        !s.contains('\''),
        "path contains a single quote and cannot be passed through /bin/sh: {s}"
    );
    format!("'{s}'")
}

/// Exit code the wrapper uses for "I could not set the limit". Distinguishing
/// it from pq's own failures is the difference between a real result and a
/// test that never reached its subject.
const WRAPPER_FAILED: i32 = 111;

/// Run pq with a hard file-size limit of `blocks` 512-byte blocks.
///
/// `trap '' XFSZ` sets `SIGXFSZ` to `SIG_IGN`, which survives `exec`, so pq
/// sees `EFBIG` from `write(2)` and can fail through its normal error path
/// instead of being killed by a signal. Without it the process would die
/// mid-write and nothing would run the staging file's cleanup.
fn run_under_file_size_limit(blocks: u64, args: &[&str]) -> Output {
    let bin = pq_bin();
    let mut script = format!(
        "trap '' XFSZ; ulimit -f {blocks} || exit {WRAPPER_FAILED}; exec {}",
        sh_quote(bin.to_str().unwrap())
    );
    for arg in args {
        script.push(' ');
        script.push_str(&sh_quote(arg));
    }
    let out = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(&script)
        .output()
        .unwrap();
    assert_ne!(
        out.status.code(),
        Some(WRAPPER_FAILED),
        "the /bin/sh wrapper could not set `ulimit -f`; pq was never run. script: {script}"
    );
    assert_ne!(
        out.status.code(),
        Some(127),
        "the /bin/sh wrapper could not find the pq binary; pq was never run. script: {script}"
    );
    assert!(
        !out.status.success(),
        "the command under a {blocks}-block file size limit SUCCEEDED, so the \
         injector never bit and this measurement says nothing. \
         stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

/// Prove `RLIMIT_FSIZE` is enforced here before trusting any result that
/// depends on it. A filesystem or platform that ignores it would make every
/// destruction test below pass for reasons that have nothing to do with pq.
fn require_file_size_limit_enforced() {
    let dir = TempDir::new().unwrap();
    let probe = dir.path().join("probe");
    let script = format!(
        "trap '' XFSZ; ulimit -f 4 || exit {WRAPPER_FAILED}; \
         dd if=/dev/zero of={} bs=1024 count=512 2>/dev/null",
        sh_quote(probe.to_str().unwrap())
    );
    let out = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(&script)
        .output()
        .unwrap();
    assert_ne!(
        out.status.code(),
        Some(WRAPPER_FAILED),
        "/bin/sh here does not support `ulimit -f`; these guards cannot bite"
    );
    let len = fs::metadata(&probe).map(|m| m.len()).unwrap_or(0);
    assert!(
        len < 512 * 1024,
        "RLIMIT_FSIZE is not enforced here: a 512 KiB write under a 2 KiB \
         limit produced {len} bytes. The destruction guards in this file \
         cannot bite on this platform."
    );
}

// ---------------------------------------------------------------------------
// The class: a failed write must not touch the destination.
//
// Each case is driven for all three commands with the same pre-existing
// destination, and asserts on the surviving bytes. Against the pre-fix binary
// every one of these fails with `23 -> N bytes`.
// ---------------------------------------------------------------------------

const PRECIOUS: &[u8] = b"MY IRREPLACEABLE NOTES\n";

fn failed_write_case(args_for: &dyn Fn(&Path, &Path) -> Vec<String>, ext: &str, what: &str) {
    require_file_size_limit_enforced();
    let dir = TempDir::new().unwrap();
    let src = make_parquet(dir.path(), "src.parquet");
    let dest = dir.path().join(format!("precious.{ext}"));
    fs::write(&dest, PRECIOUS).unwrap();
    let before = bytes(&dest);

    // 8 blocks = 4 KiB. The rendering of 20,000 rows is orders of magnitude
    // bigger, so the failure lands well after the destination would have been
    // truncated by the old code.
    let owned = args_for(&src, &dest);
    let args: Vec<&str> = owned.iter().map(String::as_str).collect();
    let out = run_under_file_size_limit(8, &args);

    let after = bytes(&dest);
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        after == before,
        "{what}: a failed write replaced the destination, {} -> {} bytes. \
         The first 60 bytes are now {:?}. stderr: {stderr}",
        before.len(),
        after.len(),
        String::from_utf8_lossy(&after[..after.len().min(60)]),
    );
    no_litter(dir.path());
}

#[test]
fn cat_output_failed_write_leaves_the_destination_intact() {
    failed_write_case(
        &|src, dest| {
            vec![
                "cat".into(),
                src.display().to_string(),
                "-O".into(),
                dest.display().to_string(),
            ]
        },
        "jsonl",
        "cat -O",
    );
}

#[test]
fn jq_output_failed_write_leaves_the_destination_intact() {
    failed_write_case(
        &|src, dest| {
            vec![
                "jq".into(),
                src.display().to_string(),
                ".".into(),
                "-o".into(),
                dest.display().to_string(),
            ]
        },
        "jsonl",
        "jq -o",
    );
}

#[test]
fn cat_jq_output_failed_write_leaves_the_destination_intact() {
    // The shape LESSONS.md records as "the worst bug of the round": before
    // the fix this one could leave a silently emptied file.
    failed_write_case(
        &|src, dest| {
            vec![
                "cat".into(),
                src.display().to_string(),
                "--jq".into(),
                ".".into(),
                "-O".into(),
                dest.display().to_string(),
            ]
        },
        "jsonl",
        "cat --jq -O",
    );
}

#[test]
fn cat_output_csv_failed_write_leaves_the_destination_intact() {
    // A different renderer (CSV, per-row) behind the same writer.
    failed_write_case(
        &|src, dest| {
            vec![
                "cat".into(),
                src.display().to_string(),
                "-O".into(),
                dest.display().to_string(),
            ]
        },
        "csv",
        "cat -O (csv)",
    );
}

#[test]
fn cat_output_parquet_failed_write_leaves_the_destination_intact() {
    failed_write_case(
        &|src, dest| {
            vec![
                "cat".into(),
                src.display().to_string(),
                "-O".into(),
                dest.display().to_string(),
            ]
        },
        "parquet",
        "cat -O (parquet)",
    );
}

#[test]
fn cat_output_failed_write_does_not_eat_its_own_input() {
    // `-O` pointing at the input, with the write failing part way. The old
    // code truncated the input before writing a byte; `open_batches` had
    // already read it, so the rows were in memory — but the file on disk was
    // gone, and on failure it stayed gone.
    require_file_size_limit_enforced();
    let dir = TempDir::new().unwrap();
    let src = make_parquet(dir.path(), "src.parquet");
    let before = bytes(&src);

    let path = src.display().to_string();
    let out = run_under_file_size_limit(8, &["cat", &path, "-O", &path]);

    let after = bytes(&src);
    assert!(
        after == before,
        "cat -O <its own input>: a failed write destroyed the input, {} -> {} bytes. stderr: {}",
        before.len(),
        after.len(),
        String::from_utf8_lossy(&out.stderr),
    );
    no_litter(dir.path());
}

// ---------------------------------------------------------------------------
// What the NEW mechanism needs that the old one did not.
//
// LESSONS.md: "when a fix changes the mechanism, the old bug's test cases no
// longer bound the risk." `File::create(dest)` needed write permission on the
// destination *file*; `rename(2)` needs it on the *directory* and consults the
// file's mode not at all. Staging therefore walks through `chmod 444` unless
// something restores the old contract — which is what `output_guard`'s
// `ensure_writable` probe is for. These two assert that `cat`/`jq` inherited
// that protection along with the staging, rather than inheriting the hole.
// ---------------------------------------------------------------------------

fn require_enforced_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let probe = TempDir::new().unwrap();
    let file = probe.path().join("probe");
    fs::write(&file, b"x").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o444)).unwrap();
    let writable = fs::OpenOptions::new().write(true).open(&file).is_ok();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(
        !writable,
        "this process can write a mode-0444 file (running as root, or a \
         filesystem that ignores permissions); this guard cannot bite here"
    );
}

#[test]
fn cat_and_jq_refuse_a_read_only_destination() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    require_enforced_permissions();
    let dir = TempDir::new().unwrap();
    let src = make_parquet_rows(dir.path(), "src.parquet", FEW);

    for (what, argv) in [
        ("cat -O", vec!["cat", src.to_str().unwrap(), "-O"]),
        ("jq -o", vec!["jq", src.to_str().unwrap(), ".", "-o"]),
        (
            "cat --jq -O",
            vec!["cat", src.to_str().unwrap(), "--jq", ".", "-O"],
        ),
    ] {
        let dest = dir
            .path()
            .join(format!("ro_{}.jsonl", what.replace([' ', '-'], "_")));
        fs::write(&dest, PRECIOUS).unwrap();
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o444)).unwrap();
        let before = bytes(&dest);
        let before_ino = fs::metadata(&dest).unwrap().ino();

        let mut argv = argv.clone();
        argv.push(dest.to_str().unwrap());
        let out = pq().args(&argv).output().unwrap();

        let after = bytes(&dest);
        let md = fs::metadata(&dest).unwrap();
        let (mode, ino) = (md.permissions().mode() & 0o777, md.ino());
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o644)).unwrap();

        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        assert!(
            after == before,
            "{what} <chmod 444>: the destination was replaced, {} -> {} bytes \
             (mode still reads {mode:o}, which would be a lie). stderr: {stderr}",
            before.len(),
            after.len()
        );
        assert!(
            !out.status.success(),
            "{what} <chmod 444>: exited 0 after walking through a read-only \
             destination. stderr: {stderr}"
        );
        assert_eq!(
            ino, before_ino,
            "{what} <chmod 444>: the file was replaced by a different inode wearing mode 0444"
        );
    }
    no_litter(dir.path());
}

#[test]
fn cat_in_a_read_only_directory_does_not_eat_its_input() {
    // A deliberate behaviour change, recorded here so it is a decision and
    // not a surprise: `cat X -O X` inside a mode-0555 directory used to
    // succeed (`File::create` on an existing file does not need directory
    // write permission). It now fails, because staging cannot create a
    // sibling — the same refusal `select`, `merge`, `slice`, `export` and
    // `sql` already give. What must never happen either way is the input
    // being destroyed, which is what this asserts on.
    use std::os::unix::fs::PermissionsExt;
    require_enforced_permissions();
    let dir = TempDir::new().unwrap();
    let src = make_parquet_rows(dir.path(), "src.parquet", FEW);
    let before = bytes(&src);
    let path = src.to_str().unwrap().to_string();

    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o555)).unwrap();
    let out = pq().args(["cat", &path, "-O", &path]).output().unwrap();
    let after = bytes(&src);
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();

    assert!(
        after == before,
        "cat -O <itself> in a 0555 dir changed the input, {} -> {} bytes. stderr: {}",
        before.len(),
        after.len(),
        String::from_utf8_lossy(&out.stderr)
    );
    no_litter(dir.path());
}

// ---------------------------------------------------------------------------
// Format sniffing: the destination the user typed decides, never the staging
// name.
//
// `output_guard` builds the staging name from the *resolved symlink target*,
// so for `-O link.parquet` where `link.parquet -> target.csv` the staging file
// is called `.target.csv-pq-tmp-<token>.csv`. A writer that sniffed the path
// it was handed would write CSV under a `.parquet` name and exit 0 — the
// confirmed `sql -o` corruption, one layer down. These assert on the magic
// bytes of the file the symlink points at.
// ---------------------------------------------------------------------------

fn linked_dest(dir: &Path, link_name: &str, target_name: &str) -> (PathBuf, PathBuf) {
    let target = dir.join(target_name);
    fs::write(&target, b"replace me\n").unwrap();
    let link = dir.join(link_name);
    std::os::unix::fs::symlink(&target, &link).unwrap();
    (link, target)
}

#[test]
fn cat_output_format_comes_from_the_destination_name_not_the_staging_name() {
    let dir = TempDir::new().unwrap();
    let src = make_parquet_rows(dir.path(), "src.parquet", FEW);
    let (link, target) = linked_dest(dir.path(), "link.parquet", "target.csv");

    pq().args(["cat", src.to_str().unwrap(), "-O", link.to_str().unwrap()])
        .assert()
        .success();

    let written = bytes(&target);
    assert_eq!(
        &written[..4],
        b"PAR1",
        "cat -O link.parquet (-> target.csv) wrote {:?}... — the staging name's \
         .csv extension was sniffed instead of the .parquet the user typed",
        String::from_utf8_lossy(&written[..written.len().min(40)])
    );
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink(),
        "the symlink was replaced by a regular file"
    );
    no_litter(dir.path());
}

#[test]
fn cat_output_csv_under_a_parquet_staging_name_still_writes_csv() {
    // The reverse direction, so the guard cannot be satisfied by a rule that
    // just always picks Parquet.
    let dir = TempDir::new().unwrap();
    let src = make_parquet_rows(dir.path(), "src.parquet", FEW);
    let (link, target) = linked_dest(dir.path(), "link.csv", "target.parquet");

    pq().args([
        "cat",
        src.to_str().unwrap(),
        "--limit",
        "2",
        "-O",
        link.to_str().unwrap(),
    ])
    .assert()
    .success();

    let text = String::from_utf8(bytes(&target)).expect("wrote binary where CSV was asked for");
    assert!(
        text.starts_with("id,name,note\n"),
        "cat -O link.csv (-> target.parquet) did not write CSV: {:?}",
        &text[..text.len().min(60)]
    );
    no_litter(dir.path());
}

#[test]
fn jq_output_format_comes_from_the_destination_name_not_the_staging_name() {
    let dir = TempDir::new().unwrap();
    let src = make_parquet_rows(dir.path(), "src.parquet", FEW);
    let (link, target) = linked_dest(dir.path(), "link.parquet", "target.csv");

    pq().args([
        "jq",
        src.to_str().unwrap(),
        ".",
        "-o",
        link.to_str().unwrap(),
    ])
    .assert()
    .success();

    let written = bytes(&target);
    assert_eq!(
        &written[..4],
        b"PAR1",
        "jq -o link.parquet (-> target.csv) wrote {:?}... — the staging name won",
        String::from_utf8_lossy(&written[..written.len().min(40)])
    );
    no_litter(dir.path());
}

#[test]
fn cat_jq_output_format_comes_from_the_destination_name_not_the_staging_name() {
    let dir = TempDir::new().unwrap();
    let src = make_parquet_rows(dir.path(), "src.parquet", FEW);
    let (link, target) = linked_dest(dir.path(), "link.parquet", "target.csv");

    pq().args([
        "cat",
        src.to_str().unwrap(),
        "--jq",
        ".",
        "-O",
        link.to_str().unwrap(),
    ])
    .assert()
    .success();

    let written = bytes(&target);
    assert_eq!(
        &written[..4],
        b"PAR1",
        "cat --jq -O link.parquet (-> target.csv) wrote {:?}... — the staging name won",
        String::from_utf8_lossy(&written[..written.len().min(40)])
    );
    no_litter(dir.path());
}

// ---------------------------------------------------------------------------
// Controls. Without these, every guard above would be satisfied by a change
// that simply refused to write anything.
// ---------------------------------------------------------------------------

#[test]
fn control_a_successful_write_still_replaces_a_pre_existing_destination() {
    let dir = TempDir::new().unwrap();
    let src = make_parquet_rows(dir.path(), "src.parquet", FEW);

    for (what, argv) in [
        ("cat -O", vec!["cat", src.to_str().unwrap(), "-O"]),
        ("jq -o", vec!["jq", src.to_str().unwrap(), ".", "-o"]),
        (
            "cat --jq -O",
            vec!["cat", src.to_str().unwrap(), "--jq", ".", "-O"],
        ),
    ] {
        let dest = dir
            .path()
            .join(format!("{}.jsonl", what.replace([' ', '-'], "_")));
        fs::write(&dest, PRECIOUS).unwrap();
        let mut argv = argv.clone();
        argv.push(dest.to_str().unwrap());
        pq().args(&argv).assert().success();

        let text = String::from_utf8(bytes(&dest)).unwrap();
        assert_ne!(
            text.as_bytes(),
            PRECIOUS,
            "{what}: a successful write did not replace the destination"
        );
        assert_eq!(
            text.lines().count(),
            FEW,
            "{what}: wrong row count in {text:?}"
        );
    }
    no_litter(dir.path());
}

#[test]
fn control_in_place_output_still_works() {
    // `cat X -O X` reads eagerly before writing, so it was safe by accident
    // before the guard and must stay safe with it.
    let dir = TempDir::new().unwrap();
    let src = make_parquet_rows(dir.path(), "src.parquet", 10);
    assert_eq!(count_rows(&src), 10);

    let path = src.to_str().unwrap().to_string();
    pq().args(["cat", &path, "--limit", "5", "-O", &path])
        .assert()
        .success();
    assert_eq!(count_rows(&src), 5, "cat -O <itself> lost rows");

    // And the jq flavours, whose values are also fully materialised first.
    pq().args(["cat", &path, "--jq", ".", "-O", &path])
        .assert()
        .success();
    assert_eq!(count_rows(&src), 5, "cat --jq -O <itself> lost rows");

    pq().args(["jq", &path, ".", "-o", &path])
        .assert()
        .success();
    assert_eq!(count_rows(&src), 5, "jq -o <itself> lost rows");

    no_litter(dir.path());
}

#[test]
fn control_a_fifo_destination_still_works() {
    // `output_guard` declines to stage over a non-regular destination and
    // writes through instead. Renaming a regular file over a fifo would
    // destroy the fifo and deliver nothing to the reader.
    let dir = TempDir::new().unwrap();
    let src = make_parquet_rows(dir.path(), "src.parquet", FEW);
    let fifo = dir.path().join("sink");
    let captured = dir.path().join("captured.jsonl");

    let status = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(status.success(), "mkfifo failed; this control cannot run");

    let script = format!(
        "cat {fifo} > {captured} & {pq} cat {src} -O {fifo}; rc=$?; wait; exit $rc",
        fifo = sh_quote(fifo.to_str().unwrap()),
        captured = sh_quote(captured.to_str().unwrap()),
        pq = sh_quote(pq_bin().to_str().unwrap()),
        src = sh_quote(src.to_str().unwrap()),
    );
    let out = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(&script)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "cat -O <fifo> failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        fs::symlink_metadata(&fifo).unwrap().file_type().is_fifo(),
        "the fifo was replaced by a regular file"
    );
    let text = String::from_utf8(bytes(&captured)).unwrap();
    assert_eq!(
        text.lines().count(),
        FEW,
        "the fifo reader did not receive the rows: {text:?}"
    );
    no_litter(dir.path());
}

#[test]
fn control_dev_stdout_still_works_redirected_and_piped() {
    // `/dev/stdout` is an alias for a descriptor the process already holds,
    // not a name in a directory. When the shell has redirected it to a
    // regular file, `fs::metadata` says "regular file", so a naive staging
    // attempt tries to create a sibling inside `/dev/fd` and the whole
    // command dies with "cannot create a temporary file next to /dev/stdout".
    // Both shapes worked before `cat`/`jq` were staged and must still work.
    let dir = TempDir::new().unwrap();
    let src = make_parquet_rows(dir.path(), "src.parquet", FEW);
    let redirected = dir.path().join("redirected.jsonl");
    let pqb = sh_quote(pq_bin().to_str().unwrap());
    let srcq = sh_quote(src.to_str().unwrap());
    let outq = sh_quote(redirected.to_str().unwrap());

    for (what, cmd) in [
        (
            "cat -O /dev/stdout > file",
            format!("{pqb} cat {srcq} -O /dev/stdout > {outq}"),
        ),
        (
            "jq -o /dev/stdout > file",
            format!("{pqb} jq {srcq} . -o /dev/stdout > {outq}"),
        ),
        (
            "cat --jq -O /dev/stdout > file",
            format!("{pqb} cat {srcq} --jq . -O /dev/stdout > {outq}"),
        ),
        (
            "cat -O /dev/stdout | cat > file",
            format!("{pqb} cat {srcq} -O /dev/stdout | cat > {outq}"),
        ),
    ] {
        let _ = fs::remove_file(&redirected);
        let out = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(&cmd)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{what} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8(bytes(&redirected)).unwrap();
        assert_eq!(text.lines().count(), FEW, "{what}: got {text:?}");
    }
    no_litter(dir.path());
}
