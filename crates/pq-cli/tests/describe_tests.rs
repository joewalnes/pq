//! `pq stats --describe a.parquet b.parquet` concatenates rows across all
//! given files, but it looked up each column by *position* in the schema
//! it read from the FIRST file only — `all_batches` (batches from every
//! file, pooled together) was then indexed by that first schema's column
//! count. A later file with fewer columns made `b.column(col_idx)` index
//! past the end of that batch's own column list and panic, instead of
//! producing an error naming the mismatched file.

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn pq() -> Command {
    assert_cmd::cargo_bin_cmd!("pq")
}

/// Import one JSONL file (one JSON object per line) into a parquet file at
/// `dir/<name>.parquet`. Everything lives under the caller's `TempDir`.
fn import_jsonl(dir: &Path, name: &str, jsonl: &str) -> PathBuf {
    let src = dir.join(format!("{name}.src.jsonl"));
    fs::write(&src, jsonl).unwrap();
    let out = dir.join(format!("{name}.parquet"));
    pq().args(["import", src.to_str().unwrap(), "-o", out.to_str().unwrap()])
        .assert()
        .success();
    out
}

/// A three-column file followed by a one-column file: `describe` walks
/// column indices 0..3 (from the first file's schema) against batches
/// pooled from both files, so it reaches into the second file's batch at
/// index 1 and 2, which don't exist.
#[test]
fn describe_wide_then_narrow_files_errors_cleanly_not_panics() {
    let dir = TempDir::new().unwrap();
    let wide = import_jsonl(
        dir.path(),
        "wide",
        "{\"a\": 1, \"b\": \"x\", \"c\": 1.5}\n{\"a\": 2, \"b\": \"y\", \"c\": 2.5}\n",
    );
    let narrow = import_jsonl(dir.path(), "narrow", "{\"a\": 10}\n{\"a\": 20}\n");

    let assert = pq()
        .args([
            "stats",
            "--describe",
            wide.to_str().unwrap(),
            narrow.to_str().unwrap(),
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert!(
        !stderr.to_lowercase().contains("panic"),
        "must fail with a clean error, not a panic: {stderr}"
    );
    assert!(
        stderr.contains("narrow") && stderr.contains("wide"),
        "error should name both mismatched files: {stderr}"
    );
}

/// Same mismatch, reverse order: the panic in the unfixed code only showed
/// up once the *first* file had more columns than a later one, so also
/// cover narrow-then-wide to make sure the guard isn't accidentally
/// order-dependent (e.g. only checking `files[0]` explicitly rather than
/// comparing schemas pairwise as they're read).
#[test]
fn describe_narrow_then_wide_files_errors_cleanly_not_panics() {
    let dir = TempDir::new().unwrap();
    let narrow = import_jsonl(dir.path(), "narrow", "{\"a\": 10}\n{\"a\": 20}\n");
    let wide = import_jsonl(
        dir.path(),
        "wide",
        "{\"a\": 1, \"b\": \"x\", \"c\": 1.5}\n{\"a\": 2, \"b\": \"y\", \"c\": 2.5}\n",
    );

    let assert = pq()
        .args([
            "stats",
            "--describe",
            narrow.to_str().unwrap(),
            wide.to_str().unwrap(),
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert!(
        !stderr.to_lowercase().contains("panic"),
        "must fail with a clean error, not a panic: {stderr}"
    );
}

/// Same file list twice (identical schema) must still work — the guard
/// must not reject legitimately matching multi-file input.
#[test]
fn describe_matching_schemas_across_files_still_works() {
    let dir = TempDir::new().unwrap();
    let a = import_jsonl(dir.path(), "a", "{\"a\": 1, \"b\": \"x\"}\n");
    let b = import_jsonl(dir.path(), "b", "{\"a\": 2, \"b\": \"y\"}\n");

    pq().args([
        "stats",
        "--describe",
        a.to_str().unwrap(),
        b.to_str().unwrap(),
    ])
    .assert()
    .success();
}
