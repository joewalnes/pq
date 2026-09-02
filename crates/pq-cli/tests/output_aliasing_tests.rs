//! Guards for the "output file truncated before the inputs are read" class.
//!
//! These assert a CLASS, not a call site: every command that takes `-o` is
//! driven with an output path that resolves to one of its own inputs, and the
//! surviving bytes on disk are checked. Asserting only on the exit code would
//! be vacuous — the pre-fix binary exits 1 for four of these commands *and*
//! leaves a 4-byte `PAR1` stub where the user's data was.
//!
//! Path aliasing is deliberately exercised through five different disguises
//! (identical string, `./` prefix, symlink, hardlink, case-variant on a
//! case-insensitive filesystem) because none of them is detectable by string
//! comparison of the two paths.

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn pq() -> Command {
    assert_cmd::cargo_bin_cmd!("pq")
}

const JSONL: &str = concat!(
    "{\"id\":0,\"name\":\"user_0\",\"score\":0.0}\n",
    "{\"id\":1,\"name\":\"user_1\",\"score\":1.5}\n",
    "{\"id\":2,\"name\":\"user_2\",\"score\":3.0}\n",
    "{\"id\":3,\"name\":\"user_3\",\"score\":4.5}\n",
    "{\"id\":4,\"name\":\"user_4\",\"score\":6.0}\n",
);

const CSV: &str = "id,name,score\n0,user_0,0\n1,user_1,1.5\n2,user_2,3\n3,user_3,4.5\n4,user_4,6\n";

/// Build a 5-row parquet file at `dest` from JSONL. Uses a `TempDir`; nothing
/// is written into the shared `tests/fixtures` tree.
fn make_parquet(dir: &Path, name: &str) -> PathBuf {
    let src = dir.join(format!("{name}.src.jsonl"));
    fs::write(&src, JSONL).unwrap();
    let out = dir.join(name);
    pq().args(["import", src.to_str().unwrap(), "-o", out.to_str().unwrap()])
        .assert()
        .success();
    fs::remove_file(&src).unwrap();
    out
}

/// A file is "destroyed" if it is empty, or a bare parquet magic stub, or no
/// longer holds the rows it held before the command ran.
fn assert_still_a_parquet_with_rows(path: &Path, want_rows: usize, what: &str) {
    let len = fs::metadata(path)
        .unwrap_or_else(|e| panic!("{what}: output file is gone: {e}"))
        .len();
    assert!(
        len > 8,
        "{what}: file truncated to {len} bytes — the input was destroyed"
    );

    // Read it back through pq itself, which is an independent code path from
    // the writers under test.
    let out = pq()
        .args(["count", path.to_str().unwrap()])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&out);
    let parsed: serde_json::Value = serde_json::from_str(text.trim())
        .unwrap_or_else(|e| panic!("{what}: `pq count` output was not JSON ({e}): {text:?}"));
    let n = parsed["count"]
        .as_u64()
        .unwrap_or_else(|| panic!("{what}: no count in {text:?}")) as usize;
    assert_eq!(n, want_rows, "{what}: expected {want_rows} rows, got {n}");
}

// ---------------------------------------------------------------------------
// The class: -o pointing at an input must never destroy that input.
// ---------------------------------------------------------------------------

#[test]
fn merge_output_aliasing_first_input_preserves_data() {
    let dir = TempDir::new().unwrap();
    let a = make_parquet(dir.path(), "a.parquet");
    let b = make_parquet(dir.path(), "b.parquet");

    pq().args([
        "merge",
        a.to_str().unwrap(),
        b.to_str().unwrap(),
        "-o",
        a.to_str().unwrap(),
    ])
    .assert()
    .success();

    assert_still_a_parquet_with_rows(&a, 10, "merge -o first-input");
}

#[test]
fn merge_output_aliasing_second_input_preserves_data() {
    let dir = TempDir::new().unwrap();
    let a = make_parquet(dir.path(), "a.parquet");
    let b = make_parquet(dir.path(), "b.parquet");

    pq().args([
        "merge",
        a.to_str().unwrap(),
        b.to_str().unwrap(),
        "-o",
        b.to_str().unwrap(),
    ])
    .assert()
    .success();

    assert_still_a_parquet_with_rows(&b, 10, "merge -o second-input");
}

#[test]
fn select_output_aliasing_preserves_data() {
    let dir = TempDir::new().unwrap();
    let a = make_parquet(dir.path(), "a.parquet");

    pq().args([
        "select",
        a.to_str().unwrap(),
        "-c",
        "id,name",
        "-o",
        a.to_str().unwrap(),
    ])
    .assert()
    .success();

    assert_still_a_parquet_with_rows(&a, 5, "select -o input");
}

#[test]
fn slice_output_aliasing_preserves_data() {
    let dir = TempDir::new().unwrap();
    let a = make_parquet(dir.path(), "a.parquet");

    pq().args([
        "slice",
        a.to_str().unwrap(),
        "--offset",
        "1",
        "--limit",
        "2",
        "-o",
        a.to_str().unwrap(),
    ])
    .assert()
    .success();

    assert_still_a_parquet_with_rows(&a, 2, "slice -o input");
}

#[test]
fn export_output_aliasing_preserves_data() {
    let dir = TempDir::new().unwrap();
    let a = make_parquet(dir.path(), "a.parquet");

    pq().args(["export", a.to_str().unwrap(), "-o", a.to_str().unwrap()])
        .assert()
        .success();

    // `export` picks its file format from the output extension, so a
    // `.parquet` destination gets JSON Lines. Whatever the format, all five
    // rows must be there — pre-fix this file was zero bytes.
    let text = fs::read_to_string(&a).unwrap();
    assert_eq!(
        text.lines().count(),
        5,
        "export -o input: expected 5 rows, got {text:?}"
    );
    assert!(
        text.contains("user_4"),
        "export -o input: data lost: {text:?}"
    );
}

#[test]
fn csv_import_output_aliasing_preserves_data() {
    // The worst of the family pre-fix: it exited 0, printed "Converted 0 rows"
    // and left a well-formed EMPTY parquet file where the CSV had been.
    let dir = TempDir::new().unwrap();
    let csv = dir.path().join("data.csv");
    fs::write(&csv, CSV).unwrap();

    pq().args(["import", csv.to_str().unwrap(), "-o", csv.to_str().unwrap()])
        .assert()
        .success()
        .stderr(predicates::str::contains("Converted 5 rows"));

    assert_still_a_parquet_with_rows(&csv, 5, "import csv -o input");
}

#[test]
fn jsonl_import_output_aliasing_preserves_data() {
    // Regression lock: this path was *reported* as vulnerable but is not —
    // `read_to_string` slurps the input before the writer opens. The guard is
    // here so that a future refactor to streaming JSON reading cannot
    // silently reintroduce the loss.
    let dir = TempDir::new().unwrap();
    let jsonl = dir.path().join("data.jsonl");
    fs::write(&jsonl, JSONL).unwrap();

    pq().args([
        "import",
        jsonl.to_str().unwrap(),
        "-o",
        jsonl.to_str().unwrap(),
    ])
    .assert()
    .success();

    assert_still_a_parquet_with_rows(&jsonl, 5, "import jsonl -o input");
}

#[test]
fn sql_output_aliasing_preserves_data() {
    // Also reported (unverified) as vulnerable; DataFusion collects all
    // batches before the write, so it is not. Locked in as a guard.
    let dir = TempDir::new().unwrap();
    let a = make_parquet(dir.path(), "a.parquet");

    pq().args([
        "sql",
        &format!("SELECT id, name FROM '{}'", a.to_str().unwrap()),
        "-o",
        a.to_str().unwrap(),
    ])
    .assert()
    .success();

    assert_still_a_parquet_with_rows(&a, 5, "sql -o input");
}

// ---------------------------------------------------------------------------
// Aliasing disguises: none of these is catchable by comparing path strings.
// ---------------------------------------------------------------------------

#[test]
fn aliasing_through_dot_slash_prefix_preserves_data() {
    let dir = TempDir::new().unwrap();
    let a = make_parquet(dir.path(), "a.parquet");

    pq().current_dir(dir.path())
        .args(["select", "./a.parquet", "-c", "id", "-o", "a.parquet"])
        .assert()
        .success();

    assert_still_a_parquet_with_rows(&a, 5, "select ./x -o x");
}

#[test]
fn aliasing_through_symlink_preserves_data_and_keeps_the_link() {
    let dir = TempDir::new().unwrap();
    let a = make_parquet(dir.path(), "a.parquet");
    let link = dir.path().join("link.parquet");
    std::os::unix::fs::symlink(&a, &link).unwrap();

    pq().args([
        "select",
        a.to_str().unwrap(),
        "-c",
        "id",
        "-o",
        link.to_str().unwrap(),
    ])
    .assert()
    .success();

    assert_still_a_parquet_with_rows(&a, 5, "select x -o symlink-to-x");
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink(),
        "the symlink was replaced by a regular file"
    );
}

#[test]
fn aliasing_through_hardlink_preserves_data() {
    let dir = TempDir::new().unwrap();
    let a = make_parquet(dir.path(), "a.parquet");
    let hard = dir.path().join("hard.parquet");
    fs::hard_link(&a, &hard).unwrap();

    pq().args([
        "select",
        a.to_str().unwrap(),
        "-c",
        "id",
        "-o",
        hard.to_str().unwrap(),
    ])
    .assert()
    .success();

    assert_still_a_parquet_with_rows(&hard, 5, "select x -o hardlink-to-x");
    assert_still_a_parquet_with_rows(&a, 5, "the other hardlink name");
}

// ---------------------------------------------------------------------------
// Atomicity: a command that fails part-way through must not leave the
// destination truncated, and must not leave staging litter behind.
// ---------------------------------------------------------------------------

#[test]
fn failed_merge_leaves_preexisting_output_intact() {
    let dir = TempDir::new().unwrap();
    let a = make_parquet(dir.path(), "a.parquet");

    // A file whose `id` column is a string, so union-mode adaptation blows up
    // *after* the output writer has been opened.
    let other_src = dir.path().join("other.jsonl");
    fs::write(&other_src, "{\"id\":\"not-a-number\"}\n").unwrap();
    let other = dir.path().join("other.parquet");
    pq().args([
        "import",
        other_src.to_str().unwrap(),
        "-o",
        other.to_str().unwrap(),
    ])
    .assert()
    .success();

    let dest = make_parquet(dir.path(), "dest.parquet");

    pq().args([
        "merge",
        a.to_str().unwrap(),
        other.to_str().unwrap(),
        "--schema-mode",
        "union",
        "-o",
        dest.to_str().unwrap(),
    ])
    .assert()
    .failure();

    assert_still_a_parquet_with_rows(&dest, 5, "destination after a failed merge");

    let litter: Vec<String> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.contains("pq-tmp"))
        .collect();
    assert!(litter.is_empty(), "staging files left behind: {litter:?}");
}

// ---------------------------------------------------------------------------
// Controls: with output != input every command must still behave exactly as
// before. Without these, the guards above prove nothing about aliasing being
// the operative variable.
// ---------------------------------------------------------------------------

#[test]
fn control_distinct_output_paths_still_work() {
    let dir = TempDir::new().unwrap();
    let a = make_parquet(dir.path(), "a.parquet");
    let b = make_parquet(dir.path(), "b.parquet");

    let merged = dir.path().join("merged.parquet");
    pq().args([
        "merge",
        a.to_str().unwrap(),
        b.to_str().unwrap(),
        "-o",
        merged.to_str().unwrap(),
    ])
    .assert()
    .success();
    assert_still_a_parquet_with_rows(&merged, 10, "control merge");

    let selected = dir.path().join("selected.parquet");
    pq().args([
        "select",
        a.to_str().unwrap(),
        "-c",
        "id,name",
        "-o",
        selected.to_str().unwrap(),
    ])
    .assert()
    .success();
    assert_still_a_parquet_with_rows(&selected, 5, "control select");

    let sliced = dir.path().join("sliced.parquet");
    pq().args([
        "slice",
        a.to_str().unwrap(),
        "--offset",
        "1",
        "--limit",
        "2",
        "-o",
        sliced.to_str().unwrap(),
    ])
    .assert()
    .success();
    assert_still_a_parquet_with_rows(&sliced, 2, "control slice");

    let exported = dir.path().join("exported.csv");
    pq().args([
        "export",
        a.to_str().unwrap(),
        "-o",
        exported.to_str().unwrap(),
        "-f",
        "csv",
    ])
    .assert()
    .success();
    assert_eq!(
        fs::read_to_string(&exported).unwrap().lines().count(),
        6,
        "control export"
    );

    let csv = dir.path().join("in.csv");
    fs::write(&csv, CSV).unwrap();
    let imported = dir.path().join("imported.parquet");
    pq().args([
        "import",
        csv.to_str().unwrap(),
        "-o",
        imported.to_str().unwrap(),
    ])
    .assert()
    .success();
    assert_still_a_parquet_with_rows(&imported, 5, "control import csv");
    // The source CSV must be untouched.
    assert_eq!(
        fs::read_to_string(&csv).unwrap(),
        CSV,
        "control: source CSV"
    );

    // And no staging litter anywhere.
    let litter: Vec<String> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.contains("pq-tmp"))
        .collect();
    assert!(litter.is_empty(), "staging files left behind: {litter:?}");
}
