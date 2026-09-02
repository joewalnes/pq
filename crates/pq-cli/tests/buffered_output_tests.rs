//! Guards for the buffered-writer change to `export`'s and `cat`'s output
//! paths (file and stdout).
//!
//! Buffering changes *when* bytes reach the destination, not *whether* they
//! do — but only if every write path flushes on the success path before
//! `pq_transform::output_guard::with_atomic_output` renames the staged file
//! over the destination. These tests exercise output sized well past every
//! internal buffer (`write_buffered`'s `BufWriter::new` default of 8 KiB and
//! `output::render_batches`'s explicit 64 KiB) so a forgotten flush — or a
//! flush placed after the rename instead of before it — would show up as
//! truncated or missing trailing rows, not as a subtle byte here and there.

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn pq() -> Command {
    assert_cmd::cargo_bin_cmd!("pq")
}

/// Row count large enough that JSONL/CSV output is several times bigger than
/// every buffer in the write path (8 KiB and 64 KiB) but the test still runs
/// in well under a second.
const ROWS: usize = 20_000;

fn make_source_jsonl(dir: &Path) -> PathBuf {
    let mut body = String::new();
    for i in 0..ROWS {
        body.push_str(&format!(
            "{{\"id\":{i},\"name\":\"user_{i}\",\"note\":\"row number {i} of {ROWS}, padded so each line has some real width\"}}\n"
        ));
    }
    let path = dir.join("source.jsonl");
    fs::write(&path, body).unwrap();
    path
}

fn make_parquet(dir: &Path) -> PathBuf {
    let src = make_source_jsonl(dir);
    let out = dir.join("source.parquet");
    pq().args(["import", src.to_str().unwrap(), "-o", out.to_str().unwrap()])
        .assert()
        .success();
    out
}

/// Every line must be present, in order, and parse as JSON — the signature
/// of a buffered write that flushed completely. A truncated tail (the
/// classic unflushed-`BufWriter` failure) would either come up short on line
/// count or leave a non-empty, non-JSON final line.
fn assert_complete_jsonl(bytes: &[u8], want_rows: usize, what: &str) {
    let text =
        String::from_utf8(bytes.to_vec()).unwrap_or_else(|e| panic!("{what}: not UTF-8: {e}"));
    assert!(
        text.ends_with('\n') || text.is_empty(),
        "{what}: output does not end with a newline, exactly what a mid-write \
         truncation looks like: tail={:?}",
        &text[text.len().saturating_sub(80)..]
    );
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), want_rows, "{what}: wrong row count");
    for (i, line) in lines.iter().enumerate() {
        let v: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|e| {
            panic!("{what}: line {i} does not parse as JSON: {e}\nline={line:?}")
        });
        assert_eq!(v["id"], i, "{what}: line {i} has the wrong id");
    }
}

#[test]
fn export_to_file_is_not_truncated_past_the_buffer_size() {
    let dir = TempDir::new().unwrap();
    let parquet = make_parquet(dir.path());
    let out = dir.path().join("out.jsonl");

    pq().args([
        "export",
        parquet.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ])
    .assert()
    .success();

    let bytes = fs::read(&out).unwrap();
    assert_complete_jsonl(&bytes, ROWS, "export -o");
}

#[test]
fn export_to_stdout_is_not_truncated_past_the_buffer_size() {
    let dir = TempDir::new().unwrap();
    let parquet = make_parquet(dir.path());

    let output = pq()
        .args(["export", parquet.to_str().unwrap(), "-f", "jsonl"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_complete_jsonl(&output, ROWS, "export to stdout");
}

#[test]
fn cat_to_stdout_is_not_truncated_past_the_buffer_size() {
    // Exercises `output::render_batches`'s internal `BufWriter`, the shape
    // `pq cat -f jsonl` on a large file takes — a separate call path from
    // `export`'s own hand-rolled stdout loop.
    let dir = TempDir::new().unwrap();
    let parquet = make_parquet(dir.path());

    let output = pq()
        .args(["cat", parquet.to_str().unwrap(), "-f", "jsonl"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_complete_jsonl(&output, ROWS, "cat -f jsonl to stdout");
}

#[test]
fn cat_to_file_is_not_truncated_past_the_buffer_size() {
    // `cat -o` goes through `write_output::write_batches_to_file`, a
    // separate call path from `export -o`'s `write_rows`.
    let dir = TempDir::new().unwrap();
    let parquet = make_parquet(dir.path());
    let out = dir.path().join("out.jsonl");

    pq().args([
        "cat",
        parquet.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ])
    .assert()
    .success();

    let bytes = fs::read(&out).unwrap();
    assert_complete_jsonl(&bytes, ROWS, "cat -o");
}

#[test]
fn export_output_is_byte_identical_across_repeated_runs() {
    // Determinism check: buffering must not introduce run-to-run
    // nondeterminism (e.g. from uninitialized buffer tail bytes leaking into
    // output, or a race between flush and rename under repeated runs).
    let dir = TempDir::new().unwrap();
    let parquet = make_parquet(dir.path());
    let out_a = dir.path().join("a.jsonl");
    let out_b = dir.path().join("b.jsonl");

    for out in [&out_a, &out_b] {
        pq().args([
            "export",
            parquet.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    }

    assert_eq!(
        fs::read(&out_a).unwrap(),
        fs::read(&out_b).unwrap(),
        "two runs of the same export produced different bytes"
    );
}

#[test]
fn a_failing_export_leaves_an_existing_destination_untouched_and_litter_free() {
    // One valid file, one that doesn't exist: `open_batches` fails partway
    // through `write_rows`'s file loop, after some rows may already be
    // sitting in the (buffered) staging file. The staged file must never be
    // renamed over the destination, and the buffer's contents must not leak
    // onto disk in its place either.
    let dir = TempDir::new().unwrap();
    let parquet = make_parquet(dir.path());
    let missing = dir.path().join("does_not_exist.parquet");
    let out = dir.path().join("out.jsonl");
    fs::write(&out, "PRE-EXISTING CONTENT\n").unwrap();

    pq().args([
        "export",
        parquet.to_str().unwrap(),
        missing.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ])
    .assert()
    .failure();

    assert_eq!(
        fs::read_to_string(&out).unwrap(),
        "PRE-EXISTING CONTENT\n",
        "destination was modified despite the export failing"
    );

    let litter: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n != "out.jsonl" && n != "source.jsonl" && n != "source.parquet")
        .collect();
    assert!(litter.is_empty(), "staging litter left behind: {litter:?}");
}
