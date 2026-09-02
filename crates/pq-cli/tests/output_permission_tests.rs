//! Guards for the "the output guard writes somewhere it had no permission to
//! write" class.
//!
//! `output_aliasing_tests.rs` covers `-o` pointing at an input. This file
//! covers the two ways the *staging* machinery itself could destroy data, both
//! of which shipped:
//!
//! * `rename(2)` needs write permission on the **directory**, not on the
//!   destination **file**, so stage-and-rename walked straight through a
//!   `chmod 444` that the previous `File::create(dest)` had respected — and
//!   then copied mode 0444 onto the replacement, so `ls -l` still showed
//!   `-r--r--r--`. Exit code 0. The mode was a lie.
//! * Any failure to create the staging file fell back to `File::create(dest)`,
//!   i.e. to the exact destructive behaviour the staging existed to prevent.
//!   Reachable through a read-only parent directory, a destination name long
//!   enough that the staging name blew past `NAME_MAX`, or stale staging litter
//!   left by a killed run.
//!
//! Every test here asserts on the **bytes on disk**, never on the exit code
//! alone: the shipped bug exited 0 while replacing the file.

use assert_cmd::Command;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn pq() -> Command {
    assert_cmd::cargo_bin_cmd!("pq")
}

const JSONL: &str = concat!(
    "{\"id\":0,\"name\":\"user_0\"}\n",
    "{\"id\":1,\"name\":\"user_1\"}\n",
    "{\"id\":2,\"name\":\"user_2\"}\n",
);

const CSV: &str = "id,name\n0,user_0\n1,user_1\n2,user_2\n";

fn make_parquet(dir: &Path, name: &str) -> PathBuf {
    // A fixed short source name: `name` may itself be 254 characters, and
    // decorating it would push the *source* over NAME_MAX before pq is
    // reached — the fixture would fail for a reason unrelated to the guard.
    let src = dir.join("make_parquet.src.jsonl");
    fs::write(&src, JSONL).unwrap();
    let out = dir.join(name);
    pq().args(["import", src.to_str().unwrap(), "-o", out.to_str().unwrap()])
        .assert()
        .success();
    fs::remove_file(&src).unwrap();
    out
}

fn chmod(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

/// Snapshot of a file's exact bytes, used to prove nothing changed.
fn bytes(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// These guards are only meaningful when the OS actually enforces the mode
/// bits against this process — as root, or on a filesystem that ignores
/// permissions, `chmod 444` is not a barrier and every assertion below would
/// pass (or fail) for reasons that have nothing to do with pq. Verify the
/// premise and blow up loudly if it does not hold, rather than reporting green
/// on a test that could not reach its subject.
fn require_enforced_permissions() {
    let probe = TempDir::new().unwrap();
    let file = probe.path().join("probe");
    fs::write(&file, b"x").unwrap();
    chmod(&file, 0o444);
    let writable = fs::OpenOptions::new().write(true).open(&file).is_ok();
    chmod(&file, 0o644);
    assert!(
        !writable,
        "this process can write a mode-0444 file (running as root, or a \
         filesystem that ignores permissions). The permission guards in this \
         file cannot bite here — run them as an unprivileged user."
    );

    let ro_dir = TempDir::new().unwrap();
    chmod(ro_dir.path(), 0o555);
    let creatable = fs::write(ro_dir.path().join("probe"), b"x").is_ok();
    chmod(ro_dir.path(), 0o755);
    assert!(
        !creatable,
        "this process can create files in a mode-0555 directory; the \
         read-only-directory guards cannot bite here."
    );
}

fn no_litter(dir: &Path) {
    let litter: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.contains("pq-tmp"))
        .collect();
    assert!(litter.is_empty(), "staging files left behind: {litter:?}");
}

// ---------------------------------------------------------------------------
// A read-only destination file must not be replaced. Whole command family.
// ---------------------------------------------------------------------------

fn read_only_dest_case(args_for: impl Fn(&Path, &Path) -> Vec<String>, ext: &str, what: &str) {
    require_enforced_permissions();
    let dir = TempDir::new().unwrap();
    let a = make_parquet(dir.path(), "a.parquet");
    let dest = dir.path().join(format!("precious.{ext}"));
    fs::write(&dest, b"PRECIOUS USER DATA THAT MUST SURVIVE\n").unwrap();
    chmod(&dest, 0o444);
    let before = bytes(&dest);

    let owned = args_for(&a, &dest);
    let args: Vec<&str> = owned.iter().map(String::as_str).collect();
    let out = pq().args(&args).output().unwrap();
    let after = bytes(&dest);
    let mode = fs::metadata(&dest).unwrap().permissions().mode() & 0o777;
    chmod(&dest, 0o644);

    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        after == before,
        "{what}: chmod 444 destination was replaced, {} -> {} bytes (mode still reads {mode:o}, which is a lie). stderr: {stderr}",
        before.len(),
        after.len()
    );
    assert!(
        !out.status.success(),
        "{what}: exited 0 after walking through chmod 444. stderr: {stderr}"
    );
    no_litter(dir.path());
}

#[test]
fn select_refuses_read_only_destination() {
    read_only_dest_case(
        |a, d| {
            vec![
                "select".into(),
                a.display().to_string(),
                "-c".into(),
                "id".into(),
                "-o".into(),
                d.display().to_string(),
            ]
        },
        "parquet",
        "select -o <chmod 444>",
    );
}

#[test]
fn slice_refuses_read_only_destination() {
    read_only_dest_case(
        |a, d| {
            vec![
                "slice".into(),
                a.display().to_string(),
                "--offset".into(),
                "1".into(),
                "--limit".into(),
                "1".into(),
                "-o".into(),
                d.display().to_string(),
            ]
        },
        "parquet",
        "slice -o <chmod 444>",
    );
}

#[test]
fn merge_refuses_read_only_destination() {
    read_only_dest_case(
        |a, d| {
            vec![
                "merge".into(),
                a.display().to_string(),
                a.display().to_string(),
                "-o".into(),
                d.display().to_string(),
            ]
        },
        "parquet",
        "merge -o <chmod 444>",
    );
}

#[test]
fn export_refuses_read_only_destination() {
    read_only_dest_case(
        |a, d| {
            vec![
                "export".into(),
                a.display().to_string(),
                "-o".into(),
                d.display().to_string(),
            ]
        },
        "csv",
        "export -o <chmod 444>",
    );
}

#[test]
fn sql_refuses_read_only_destination() {
    read_only_dest_case(
        |a, d| {
            vec![
                "sql".into(),
                format!("SELECT id FROM '{}'", a.display()),
                "-o".into(),
                d.display().to_string(),
            ]
        },
        "parquet",
        "sql -o <chmod 444>",
    );
}

#[test]
fn import_refuses_read_only_destination() {
    require_enforced_permissions();
    let dir = TempDir::new().unwrap();
    let csv = dir.path().join("in.csv");
    fs::write(&csv, CSV).unwrap();
    let dest = dir.path().join("precious.parquet");
    fs::write(&dest, b"PRECIOUS USER DATA THAT MUST SURVIVE\n").unwrap();
    chmod(&dest, 0o444);
    let before = bytes(&dest);

    let out = pq()
        .args([
            "import",
            csv.to_str().unwrap(),
            "-o",
            dest.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let after = bytes(&dest);
    chmod(&dest, 0o644);

    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        after == before,
        "import -o <chmod 444>: destination replaced, {} -> {} bytes. stderr: {stderr}",
        before.len(),
        after.len()
    );
    assert!(!out.status.success(), "import -o <chmod 444>: exited 0");
    no_litter(dir.path());
}

// ---------------------------------------------------------------------------
// A read-only *parent directory* must not be worked around by writing the
// destination in place. This is the trigger that kept the original data-loss
// bug fully alive: `create_new` in a 0555 directory fails, and the pre-fix
// fallback then handed `File::create` the user's own input.
// ---------------------------------------------------------------------------

/// Build a directory holding `files`, chmod it 0555, run `args`, restore 0755.
fn in_read_only_dir(dir: &TempDir, args: &[String], dest: &Path) -> (bool, String, Vec<u8>) {
    require_enforced_permissions();
    chmod(dir.path(), 0o555);
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = pq().args(&argv).output().unwrap();
    let after = bytes(dest);
    chmod(dir.path(), 0o755);
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        after,
    )
}

#[test]
fn csv_import_in_read_only_dir_does_not_eat_the_csv() {
    // The single worst observed behaviour of the whole round: exit 0,
    // "Converted 0 rows", and the user's CSV replaced by an empty parquet.
    let dir = TempDir::new().unwrap();
    let csv = dir.path().join("data.csv");
    fs::write(&csv, CSV).unwrap();
    let before = bytes(&csv);

    let args = vec![
        "import".to_string(),
        csv.display().to_string(),
        "-o".to_string(),
        csv.display().to_string(),
    ];
    let (ok, stderr, after) = in_read_only_dir(&dir, &args, &csv);

    assert!(
        after == before,
        "import in a 0555 dir replaced the CSV, {} -> {} bytes. stderr: {stderr}",
        before.len(),
        after.len()
    );
    assert!(!ok, "import in a 0555 dir exited 0. stderr: {stderr}");
    no_litter(dir.path());
}

#[test]
fn select_in_read_only_dir_does_not_eat_its_input() {
    let dir = TempDir::new().unwrap();
    let a = make_parquet(dir.path(), "a.parquet");
    let before = bytes(&a);

    let args = vec![
        "select".to_string(),
        a.display().to_string(),
        "-c".to_string(),
        "id".to_string(),
        "-o".to_string(),
        a.display().to_string(),
    ];
    let (ok, stderr, after) = in_read_only_dir(&dir, &args, &a);

    assert!(
        after == before,
        "select in a 0555 dir replaced its input, {} -> {} bytes. stderr: {stderr}",
        before.len(),
        after.len()
    );
    assert!(!ok, "select in a 0555 dir exited 0. stderr: {stderr}");
    no_litter(dir.path());
}

#[test]
fn slice_in_read_only_dir_does_not_eat_its_input() {
    let dir = TempDir::new().unwrap();
    let a = make_parquet(dir.path(), "a.parquet");
    let before = bytes(&a);

    let args = vec![
        "slice".to_string(),
        a.display().to_string(),
        "--offset".to_string(),
        "1".to_string(),
        "--limit".to_string(),
        "1".to_string(),
        "-o".to_string(),
        a.display().to_string(),
    ];
    let (ok, stderr, after) = in_read_only_dir(&dir, &args, &a);

    assert!(
        after == before,
        "slice in a 0555 dir replaced its input, {} -> {} bytes. stderr: {stderr}",
        before.len(),
        after.len()
    );
    assert!(!ok, "slice in a 0555 dir exited 0. stderr: {stderr}");
    no_litter(dir.path());
}

#[test]
fn merge_in_read_only_dir_does_not_eat_its_input() {
    let dir = TempDir::new().unwrap();
    let a = make_parquet(dir.path(), "a.parquet");
    let b = make_parquet(dir.path(), "b.parquet");
    let before = bytes(&a);

    let args = vec![
        "merge".to_string(),
        a.display().to_string(),
        b.display().to_string(),
        "-o".to_string(),
        a.display().to_string(),
    ];
    let (ok, stderr, after) = in_read_only_dir(&dir, &args, &a);

    assert!(
        after == before,
        "merge in a 0555 dir replaced its input, {} -> {} bytes. stderr: {stderr}",
        before.len(),
        after.len()
    );
    assert!(!ok, "merge in a 0555 dir exited 0. stderr: {stderr}");
    no_litter(dir.path());
}

#[test]
fn export_in_read_only_dir_does_not_eat_its_input() {
    let dir = TempDir::new().unwrap();
    let a = make_parquet(dir.path(), "a.parquet");
    let before = bytes(&a);

    let args = vec![
        "export".to_string(),
        a.display().to_string(),
        "-o".to_string(),
        a.display().to_string(),
    ];
    let (ok, stderr, after) = in_read_only_dir(&dir, &args, &a);

    assert!(
        after == before,
        "export in a 0555 dir replaced its input, {} -> {} bytes. stderr: {stderr}",
        before.len(),
        after.len()
    );
    assert!(!ok, "export in a 0555 dir exited 0. stderr: {stderr}");
    no_litter(dir.path());
}

#[test]
fn sql_in_read_only_dir_does_not_eat_its_input() {
    let dir = TempDir::new().unwrap();
    let a = make_parquet(dir.path(), "a.parquet");
    let before = bytes(&a);

    let args = vec![
        "sql".to_string(),
        format!("SELECT id FROM '{}'", a.display()),
        "-o".to_string(),
        a.display().to_string(),
    ];
    let (ok, stderr, after) = in_read_only_dir(&dir, &args, &a);

    assert!(
        after == before,
        "sql in a 0555 dir replaced its input, {} -> {} bytes. stderr: {stderr}",
        before.len(),
        after.len()
    );
    assert!(!ok, "sql in a 0555 dir exited 0. stderr: {stderr}");
    no_litter(dir.path());
}

// ---------------------------------------------------------------------------
// A legal-but-long destination name. 254 characters is legal everywhere pq
// runs; the pre-fix staging name added ~16 characters to the whole name,
// exceeded NAME_MAX, and the fallback then destroyed the file (833B -> 4B).
// Here the staging name is budgeted, so the operation must simply *work*.
// ---------------------------------------------------------------------------

fn long_name(ext: &str) -> String {
    let stem = "x".repeat(254 - 1 - ext.len());
    let name = format!("{stem}.{ext}");
    assert_eq!(name.len(), 254);
    name
}

#[test]
fn select_onto_a_254_char_destination_keeps_the_data() {
    let dir = TempDir::new().unwrap();
    let a = make_parquet(dir.path(), &long_name("parquet"));

    let out = pq()
        .args([
            "select",
            a.to_str().unwrap(),
            "-c",
            "id",
            "-o",
            a.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    assert!(
        fs::metadata(&a).unwrap().len() > 8,
        "254-char destination was truncated to {} bytes. stderr: {stderr}",
        fs::metadata(&a).unwrap().len()
    );
    assert!(
        out.status.success(),
        "254-char destination failed outright. stderr: {stderr}"
    );
    assert_eq!(count_rows(&a), 3, "rows lost. stderr: {stderr}");
    no_litter(dir.path());
}

#[test]
fn csv_import_onto_a_254_char_destination_keeps_the_data() {
    let dir = TempDir::new().unwrap();
    let csv = dir.path().join(long_name("csv"));
    fs::write(&csv, CSV).unwrap();

    let out = pq()
        .args(["import", csv.to_str().unwrap(), "-o", csv.to_str().unwrap()])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    assert!(
        out.status.success(),
        "254-char import failed. stderr: {stderr}"
    );
    assert_eq!(count_rows(&csv), 3, "CSV rows lost. stderr: {stderr}");
    no_litter(dir.path());
}

// ---------------------------------------------------------------------------
// Staging litter left by a killed run must be inert.
//
// The pre-fix staging name was `.{dest}-pq-tmp-{pid}-{counter}` with the
// counter always starting at 0, so on macOS (pids wrap at 99999) a killed run's
// litter armed a ~1-in-100k trap for the next process that drew the same pid —
// and springing it meant `File::create` on the destination. Names are random
// now, and a collision retries instead of falling through. The deterministic
// version of this guard is
// `output_guard::tests::occupied_staging_name_never_writes_the_destination`;
// this one drives it end-to-end through the CLI.
// ---------------------------------------------------------------------------

#[test]
fn staging_litter_does_not_arm_a_trap_for_a_later_run() {
    let dir = TempDir::new().unwrap();
    let a = make_parquet(dir.path(), "a.parquet");
    let dest = make_parquet(dir.path(), "dest.parquet");
    let dest_before = bytes(&dest);

    // Plant the entire pre-fix staging namespace this process could plausibly
    // hand a child, plus the low counter values.
    let my_pid = std::process::id();
    let mut planted = Vec::new();
    for offset in 1..=64u32 {
        let pid = (my_pid + offset) % 100_000;
        for counter in 0..2 {
            let p = dir
                .path()
                .join(format!(".dest.parquet-pq-tmp-{pid}-{counter}.parquet"));
            fs::write(&p, b"stale litter from a SIGKILLed run").unwrap();
            planted.push(p);
        }
    }

    let out = pq()
        .args([
            "select",
            a.to_str().unwrap(),
            "-c",
            "id",
            "-o",
            dest.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    if out.status.success() {
        assert_eq!(count_rows(&dest), 3, "rows lost. stderr: {stderr}");
    } else {
        assert!(
            bytes(&dest) == dest_before,
            "failed run still modified the destination. stderr: {stderr}"
        );
    }

    // We do not reap other processes' staging files: a hidden `-pq-tmp-` file
    // may belong to a pq that is still running, and deleting it would corrupt
    // that run. Litter is inert, not collected.
    for p in &planted {
        assert!(
            p.exists(),
            "pq deleted a staging file it did not create: {}",
            p.display()
        );
    }
}

// ---------------------------------------------------------------------------
// Controls. Without these the refusals above could be satisfied by a guard
// that refuses everything.
// ---------------------------------------------------------------------------

#[test]
fn control_writable_destination_in_a_writable_dir_still_works() {
    require_enforced_permissions();
    let dir = TempDir::new().unwrap();
    let a = make_parquet(dir.path(), "a.parquet");

    // Pre-existing, writable destination: must be replaced, as always.
    let dest = make_parquet(dir.path(), "dest.parquet");
    pq().args([
        "select",
        a.to_str().unwrap(),
        "-c",
        "id",
        "-o",
        dest.to_str().unwrap(),
    ])
    .assert()
    .success();
    assert_eq!(count_rows(&dest), 3);

    // Fresh destination: must be created.
    let fresh = dir.path().join("fresh.parquet");
    pq().args([
        "slice",
        a.to_str().unwrap(),
        "--offset",
        "1",
        "--limit",
        "2",
        "-o",
        fresh.to_str().unwrap(),
    ])
    .assert()
    .success();
    assert_eq!(count_rows(&fresh), 2);

    // A destination the user owns but has made read-only is refused; making it
    // writable again lets the same command through. This is the pair that
    // proves the check keys on writability, not on "the file exists".
    let toggled = make_parquet(dir.path(), "toggled.parquet");
    let before = bytes(&toggled);
    chmod(&toggled, 0o444);
    let out = pq()
        .args([
            "select",
            a.to_str().unwrap(),
            "-c",
            "id",
            "-o",
            toggled.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    chmod(&toggled, 0o644);
    assert!(
        !out.status.success(),
        "a read-only destination was accepted: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        bytes(&toggled) == before,
        "read-only destination was replaced"
    );

    pq().args([
        "select",
        a.to_str().unwrap(),
        "-c",
        "id",
        "-o",
        toggled.to_str().unwrap(),
    ])
    .assert()
    .success();
    assert_eq!(count_rows(&toggled), 3);

    no_litter(dir.path());
}

#[test]
fn control_read_only_destination_keeps_its_mode_after_a_refusal() {
    // The aggravating half of the bug: the replacement inherited mode 0444, so
    // `ls -l` still claimed the file was protected. After a refusal the mode
    // must be untouched *and* the inode must be the original one.
    use std::os::unix::fs::MetadataExt;
    require_enforced_permissions();
    let dir = TempDir::new().unwrap();
    let a = make_parquet(dir.path(), "a.parquet");
    let dest = make_parquet(dir.path(), "dest.parquet");
    chmod(&dest, 0o444);
    let before_ino = fs::metadata(&dest).unwrap().ino();

    let out = pq()
        .args([
            "select",
            a.to_str().unwrap(),
            "-c",
            "id",
            "-o",
            dest.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let md = fs::metadata(&dest).unwrap();
    let mode = md.permissions().mode() & 0o777;
    let ino = md.ino();
    chmod(&dest, 0o644);

    assert!(
        !out.status.success(),
        "a read-only destination was accepted: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(mode, 0o444, "mode changed");
    assert_eq!(
        ino, before_ino,
        "the file was replaced by a different inode wearing mode 0444"
    );
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
    parsed["count"]
        .as_u64()
        .unwrap_or_else(|| panic!("no count in {text:?}")) as usize
}
