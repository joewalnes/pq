//! Guards for the "output format decided twice, from two different strings"
//! class.
//!
//! `sql -o DEST` resolved the format from `DEST`'s extension and then handed
//! the writer the *staging* path built by `pq_transform::output_guard`. That
//! writer (`write_output::write_batches_to_file`) re-sniffed the extension of
//! the path it was given — a second, independent decision. The staging name
//! is built from `resolve_symlinks(DEST)`, i.e. from the symlink *target*'s
//! name, so whenever a link and its target disagreed about extension the
//! second sniff won and `pq sql ... -o link.parquet` wrote CSV (or JSONL)
//! into a file the user had named `.parquet`, exit 0, "Wrote N rows".
//!
//! These tests assert the CLASS, not one extension pair: several link/target
//! disagreements in both directions, dangling and relative links, plus
//! non-symlink controls proving the same commands are correct when nothing
//! disagrees. The instrument is the *bytes on disk* (Parquet's `PAR1` magic
//! vs. a CSV header line vs. a JSON `[`), read from the symlink target, plus
//! an independent readback through `pq info`. Exit codes are 0 in every
//! broken case and prove nothing.

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn pq() -> Command {
    assert_cmd::cargo_bin_cmd!("pq")
}

const JSONL: &str = "{\"id\":1,\"n\":\"x\"}\n{\"id\":2,\"n\":\"y\"}\n";

fn make_parquet(dir: &Path) -> PathBuf {
    let src = dir.join("src.jsonl");
    fs::write(&src, JSONL).unwrap();
    let out = dir.join("src.parquet");
    pq().args(["import", src.to_str().unwrap(), "-o", out.to_str().unwrap()])
        .assert()
        .success();
    out
}

/// First four bytes of a file, as the file command would look at them.
fn magic(path: &Path) -> Vec<u8> {
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    bytes.into_iter().take(4).collect()
}

fn assert_is_parquet(path: &Path, case: &str) {
    let m = magic(path);
    assert_eq!(
        m,
        b"PAR1".to_vec(),
        "[{case}] {} does not start with Parquet magic PAR1; got {:02x?} ({:?})",
        path.display(),
        m,
        String::from_utf8_lossy(&m)
    );
    // Independent readback: the footer must be a real Parquet footer, not
    // just a leading magic.
    pq().args(["info", path.to_str().unwrap()])
        .assert()
        .success();
}

fn assert_is_csv(path: &Path, case: &str) {
    let text = fs::read_to_string(path).unwrap();
    assert!(
        text.starts_with("id,n"),
        "[{case}] {} is not CSV: {:?}",
        path.display(),
        text.chars().take(40).collect::<String>()
    );
}

fn assert_is_jsonl(path: &Path, case: &str) {
    let text = fs::read_to_string(path).unwrap();
    let first = text.lines().next().unwrap_or("");
    assert!(
        first.starts_with('{') && first.ends_with('}'),
        "[{case}] {} is not JSON Lines: {first:?}",
        path.display()
    );
}

// ---------------------------------------------------------------------------
// The bug: the destination the user named must decide the format, even when a
// symlink points somewhere with a different extension.
// ---------------------------------------------------------------------------

/// `-o link.parquet` where `link.parquet -> target.csv`. Pre-fix this wrote
/// CSV bytes into `target.csv` and reported "Wrote 2 rows to .../link.parquet".
#[test]
fn sql_parquet_symlink_to_csv_target_writes_parquet() {
    let dir = TempDir::new().unwrap();
    let src = make_parquet(dir.path());
    let target = dir.path().join("target.csv");
    fs::write(&target, "").unwrap();
    let link = dir.path().join("link.parquet");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    pq().args([
        "sql",
        &format!("SELECT * FROM '{}'", src.to_str().unwrap()),
        "-o",
        link.to_str().unwrap(),
    ])
    .assert()
    .success();

    assert_is_parquet(&target, "sql -o link.parquet -> target.csv");
}

/// `-o link.parquet` where the target has *no* extension. Pre-fix the second
/// sniff fell through to its silent JSONL default.
#[test]
fn sql_parquet_symlink_to_extensionless_target_writes_parquet() {
    let dir = TempDir::new().unwrap();
    let src = make_parquet(dir.path());
    let target = dir.path().join("target-noext");
    fs::write(&target, "").unwrap();
    let link = dir.path().join("link.parquet");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    pq().args([
        "sql",
        &format!("SELECT * FROM '{}'", src.to_str().unwrap()),
        "-o",
        link.to_str().unwrap(),
    ])
    .assert()
    .success();

    assert_is_parquet(&target, "sql -o link.parquet -> target-noext");
}

/// Dangling link: the target does not exist yet, so only its *name* is
/// available to sniff — which is exactly the failure mode.
#[test]
fn sql_parquet_dangling_symlink_to_csv_name_writes_parquet() {
    let dir = TempDir::new().unwrap();
    let src = make_parquet(dir.path());
    let target = dir.path().join("missing.csv");
    let link = dir.path().join("link.parquet");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    pq().args([
        "sql",
        &format!("SELECT * FROM '{}'", src.to_str().unwrap()),
        "-o",
        link.to_str().unwrap(),
    ])
    .assert()
    .success();

    assert_is_parquet(&target, "sql -o dangling link.parquet -> missing.csv");
}

/// A *relative* symlink target resolves against the link's directory, so the
/// staging name is built from the same disagreeing extension.
#[test]
fn sql_parquet_relative_symlink_to_csv_target_writes_parquet() {
    let dir = TempDir::new().unwrap();
    let src = make_parquet(dir.path());
    let target = dir.path().join("rel-target.csv");
    fs::write(&target, "").unwrap();
    let link = dir.path().join("rel-link.parquet");
    std::os::unix::fs::symlink("rel-target.csv", &link).unwrap();

    pq().args([
        "sql",
        &format!("SELECT * FROM '{}'", src.to_str().unwrap()),
        "-o",
        link.to_str().unwrap(),
    ])
    .assert()
    .success();

    assert_is_parquet(&target, "sql -o rel-link.parquet -> rel-target.csv");
}

/// The other direction. A `.csv` destination pointing at a `.parquet` target
/// must still produce CSV — the destination name always wins, both ways.
#[test]
fn sql_csv_symlink_to_parquet_target_writes_csv() {
    let dir = TempDir::new().unwrap();
    let src = make_parquet(dir.path());
    let target = dir.path().join("target.parquet");
    fs::write(&target, "").unwrap();
    let link = dir.path().join("link.csv");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    pq().args([
        "sql",
        &format!("SELECT * FROM '{}'", src.to_str().unwrap()),
        "-o",
        link.to_str().unwrap(),
    ])
    .assert()
    .success();

    assert_is_csv(&target, "sql -o link.csv -> target.parquet");
}

/// `export` was already immune (it passes the resolved format in explicitly).
/// Guarded so it stays that way, and so this file covers the class rather
/// than the one command that happened to be broken.
#[test]
fn export_jsonl_symlink_to_csv_target_writes_jsonl() {
    let dir = TempDir::new().unwrap();
    let src = make_parquet(dir.path());
    let target = dir.path().join("target.csv");
    fs::write(&target, "").unwrap();
    let link = dir.path().join("link.jsonl");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    pq().args([
        "export",
        src.to_str().unwrap(),
        "-o",
        link.to_str().unwrap(),
    ])
    .assert()
    .success();

    assert_is_jsonl(&target, "export -o link.jsonl -> target.csv");
}

// ---------------------------------------------------------------------------
// Controls: the same commands with nothing disagreeing.
// ---------------------------------------------------------------------------

#[test]
fn control_sql_to_a_real_parquet_file_writes_parquet() {
    let dir = TempDir::new().unwrap();
    let src = make_parquet(dir.path());
    let out = dir.path().join("control.parquet");
    pq().args([
        "sql",
        &format!("SELECT * FROM '{}'", src.to_str().unwrap()),
        "-o",
        out.to_str().unwrap(),
    ])
    .assert()
    .success();
    assert_is_parquet(&out, "control: real .parquet destination");
}

#[test]
fn control_sql_symlink_with_agreeing_extension_writes_parquet() {
    let dir = TempDir::new().unwrap();
    let src = make_parquet(dir.path());
    let target = dir.path().join("agree-target.parquet");
    fs::write(&target, "").unwrap();
    let link = dir.path().join("agree-link.parquet");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    pq().args([
        "sql",
        &format!("SELECT * FROM '{}'", src.to_str().unwrap()),
        "-o",
        link.to_str().unwrap(),
    ])
    .assert()
    .success();
    assert_is_parquet(&target, "control: link and target both .parquet");
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink(),
        "the symlink was replaced by a regular file"
    );
}

#[test]
fn control_sql_to_a_real_csv_file_writes_csv() {
    let dir = TempDir::new().unwrap();
    let src = make_parquet(dir.path());
    let out = dir.path().join("control.csv");
    pq().args([
        "sql",
        &format!("SELECT * FROM '{}'", src.to_str().unwrap()),
        "-o",
        out.to_str().unwrap(),
    ])
    .assert()
    .success();
    assert_is_csv(&out, "control: real .csv destination");
}

/// An explicit `-f` still overrides a recognized extension for `sql -o` —
/// unchanged behaviour, guarded because the fix moves the resolution around.
#[test]
fn control_sql_explicit_format_still_overrides_extension() {
    let dir = TempDir::new().unwrap();
    let src = make_parquet(dir.path());
    let out = dir.path().join("out.csv");
    pq().args([
        "sql",
        &format!("SELECT * FROM '{}'", src.to_str().unwrap()),
        "-o",
        out.to_str().unwrap(),
        "-f",
        "jsonl",
    ])
    .assert()
    .success();
    assert_is_jsonl(&out, "control: -f jsonl overrides .csv");
}

// ---------------------------------------------------------------------------
// A diagnostic must not announce something that did not happen.
// ---------------------------------------------------------------------------

/// `export -f table -o out.csv` printed
/// `note: -f/--format table overrides the format implied by 'out.csv's
/// extension (csv)` and *then* failed. No override ever took effect.
#[test]
fn export_rejects_table_to_file_without_claiming_an_override() {
    let dir = TempDir::new().unwrap();
    let src = make_parquet(dir.path());
    let out = dir.path().join("out.csv");
    let assert = pq()
        .args([
            "export",
            src.to_str().unwrap(),
            "-f",
            "table",
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert!(
        stderr.contains("can't be written to a file"),
        "expected the file-format rejection, got: {stderr}"
    );
    assert!(
        !stderr.contains("overrides"),
        "the run failed but still claimed an override took effect: {stderr}"
    );
}

#[test]
fn sql_rejects_table_to_file_without_claiming_an_override() {
    let dir = TempDir::new().unwrap();
    let src = make_parquet(dir.path());
    let out = dir.path().join("out.csv");
    let assert = pq()
        .args([
            "sql",
            &format!("SELECT * FROM '{}'", src.to_str().unwrap()),
            "-f",
            "table",
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert!(
        stderr.contains("can't be written to a file"),
        "expected the file-format rejection, got: {stderr}"
    );
    assert!(
        !stderr.contains("overrides"),
        "the run failed but still claimed an override took effect: {stderr}"
    );
}

/// Control: when the override *does* take effect, the note must still be
/// printed. Without this, deleting the note entirely would pass the two
/// tests above.
#[test]
fn control_effective_override_still_prints_the_note() {
    let dir = TempDir::new().unwrap();
    let src = make_parquet(dir.path());

    let out = dir.path().join("out.csv");
    let assert = pq()
        .args([
            "export",
            src.to_str().unwrap(),
            "-f",
            "jsonl",
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert!(
        stderr.contains("overrides"),
        "export: an override that did take effect was not announced: {stderr}"
    );
    assert_is_jsonl(&out, "control: export -f jsonl -o out.csv");

    let out2 = dir.path().join("out2.csv");
    let assert = pq()
        .args([
            "sql",
            &format!("SELECT * FROM '{}'", src.to_str().unwrap()),
            "-f",
            "jsonl",
            "-o",
            out2.to_str().unwrap(),
        ])
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert!(
        stderr.contains("overrides"),
        "sql: an override that did take effect was not announced: {stderr}"
    );
}
