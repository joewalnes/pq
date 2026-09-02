//! `pq stats --describe a.parquet b.parquet` concatenates rows across all
//! given files, but it looked up each column by *position* in the schema
//! it read from the FIRST file only — `all_batches` (batches from every
//! file, pooled together) was then indexed by that first schema's column
//! count. A later file with fewer columns made `b.column(col_idx)` index
//! past the end of that batch's own column list and panic, instead of
//! producing an error naming the mismatched file.
//!
//! The fix for that panic (comparing whole `arrow::datatypes::Field`s)
//! over-corrected: `Field` equality also considers nullability and
//! per-field metadata, which `arrow::compute::concat` — the operation this
//! guard exists to protect — never looks at. `describe_differing_*_across_files_still_works`
//! below cover the resulting false rejections, and
//! `describe_type_mismatch_error_names_the_actual_difference` covers the
//! companion bug where a genuine mismatch could be reported with two
//! identical-looking column lists.

use arrow::array::{
    Int64Array, RecordBatch, StringArray, TimestampMicrosecondArray, TimestampMillisecondArray,
};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;

/// Write a single `RecordBatch` straight to a Parquet file via `pq-core`,
/// bypassing `pq import` so the caller has full control of the schema
/// (nullability, field metadata) rather than whatever `import`'s inference
/// would pick.
fn write_batch(dir: &Path, name: &str, batch: RecordBatch) -> PathBuf {
    let out = dir.join(format!("{name}.parquet"));
    pq_core::writer::write_batches(
        &out,
        std::slice::from_ref(&batch),
        &pq_core::writer::WriteOptions::default(),
    )
    .unwrap();
    out
}

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

/// Two files whose columns agree on every `DataType` but disagree on
/// nullability (one column `NOT NULL`, the other nullable) must still
/// describe together. `arrow::compute::concat`
/// (arrow-select-53.4.1/src/concat.rs:160-165) only compares `data_type()`,
/// so a guard that also rejects on nullability — as a whole-`Field`
/// comparison does, since `Field::eq` includes `nullable`
/// (arrow-schema-53.4.1/src/field.rs:52-59) — rejects input `concat` would
/// have handled fine. This is a realistic trigger: a file written with a
/// `NOT NULL` constraint (or by pyarrow/pandas) next to one without.
#[test]
fn describe_differing_nullability_across_files_still_works() {
    let dir = TempDir::new().unwrap();

    let nullable_schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, true),
        Field::new("name", DataType::Utf8, true),
    ]));
    let batch_a = RecordBatch::try_new(
        nullable_schema,
        vec![
            Arc::new(Int64Array::from(vec![1, 2])),
            Arc::new(StringArray::from(vec!["x", "y"])),
        ],
    )
    .unwrap();
    let a = write_batch(dir.path(), "nullable", batch_a);

    let non_nullable_schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("name", DataType::Utf8, false),
    ]));
    let batch_b = RecordBatch::try_new(
        non_nullable_schema,
        vec![
            Arc::new(Int64Array::from(vec![3, 4])),
            Arc::new(StringArray::from(vec!["z", "w"])),
        ],
    )
    .unwrap();
    let b = write_batch(dir.path(), "nonnull", batch_b);

    pq().args([
        "stats",
        "--describe",
        a.to_str().unwrap(),
        b.to_str().unwrap(),
    ])
    .assert()
    .success();
}

/// A genuine, unmergeable schema mismatch (`concat` really does refuse two
/// different `Timestamp` units) must name the actual difference, never
/// print the same-looking column list for both files. The friendly
/// renderer used elsewhere (`format_dtype`) collapses every `Timestamp(_,
/// _)` to the string "timestamp" regardless of unit, which is exactly the
/// bug report's "IDENTICAL column lists" failure mode; the error must use a
/// rendering that cannot collapse two different `DataType`s onto the same
/// text.
#[test]
fn describe_type_mismatch_error_names_the_actual_difference() {
    let dir = TempDir::new().unwrap();

    let millis_schema = Arc::new(Schema::new(vec![Field::new(
        "ts",
        DataType::Timestamp(TimeUnit::Millisecond, None),
        true,
    )]));
    let batch_a = RecordBatch::try_new(
        millis_schema,
        vec![Arc::new(TimestampMillisecondArray::from(vec![1_i64, 2]))],
    )
    .unwrap();
    let a = write_batch(dir.path(), "millis", batch_a);

    let micros_schema = Arc::new(Schema::new(vec![Field::new(
        "ts",
        DataType::Timestamp(TimeUnit::Microsecond, None),
        true,
    )]));
    let batch_b = RecordBatch::try_new(
        micros_schema,
        vec![Arc::new(TimestampMicrosecondArray::from(vec![3_i64, 4]))],
    )
    .unwrap();
    let b = write_batch(dir.path(), "micros", batch_b);

    let assert = pq()
        .args([
            "stats",
            "--describe",
            a.to_str().unwrap(),
            b.to_str().unwrap(),
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert!(
        !stderr.to_lowercase().contains("panic"),
        "must fail cleanly, not panic: {stderr}"
    );

    let marker = "has columns [";
    let first_start = stderr.find(marker).expect("first column list") + marker.len();
    let first_end = first_start + stderr[first_start..].find(']').expect("closing bracket");
    let first_list = &stderr[first_start..first_end];

    let second_start = first_end
        + stderr[first_end..]
            .find(marker)
            .expect("second column list")
        + marker.len();
    let second_end = second_start + stderr[second_start..].find(']').expect("closing bracket");
    let second_list = &stderr[second_start..second_end];

    assert_ne!(
        first_list, second_list,
        "error printed identical-looking column lists for a genuine, \
         unmergeable schema mismatch: {stderr}"
    );
    assert!(
        first_list.contains("Millisecond") || second_list.contains("Millisecond"),
        "error should name the actual type difference, not collapse both \
         to \"timestamp\": {stderr}"
    );
    assert!(
        first_list.contains("Microsecond") || second_list.contains("Microsecond"),
        "error should name the actual type difference, not collapse both \
         to \"timestamp\": {stderr}"
    );
}
