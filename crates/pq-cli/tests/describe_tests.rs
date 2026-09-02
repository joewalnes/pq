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
    Float64Array, Int64Array, RecordBatch, StringArray, TimestampMicrosecondArray,
    TimestampMillisecondArray,
};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use assert_cmd::Command;
use std::collections::HashMap;
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

// ---------------------------------------------------------------------------
// Column *names*, not just `DataType`s, must agree across files, and column
// *order* must not matter once the names agree.
//
// `schemas_concat_compatible` briefly compared only `DataType` per column
// position, dropping `name` from the check entirely. Two files with
// completely disjoint column names but a shared `DataType` passed that
// guard, and `run`'s position-based indexing then labelled the second
// file's data with the first file's column names — a silent wrong answer,
// not a refusal. Ground truth for every numeric assertion below is computed
// independently by hand from the literal input arrays, mirroring what
// `pyarrow.concat_tables` would report, never by comparing pq's output
// against another pq invocation.
// ---------------------------------------------------------------------------

/// Writes a single named `Int64` column, letting the test control the
/// column name independently of the file name (`write_ints` above always
/// names its column `"v"`).
fn write_named_column(dir: &Path, file_name: &str, col_name: &str, values: Vec<i64>) -> PathBuf {
    let schema = Arc::new(Schema::new(vec![Field::new(
        col_name,
        DataType::Int64,
        true,
    )]));
    let batch = RecordBatch::try_new(schema, vec![Arc::new(Int64Array::from(values))]).unwrap();
    write_batch(dir, file_name, batch)
}

/// The exact bug: two files, disjoint column names, both `Int64`. Before
/// this fix, `alpha`'s and `omega`'s values were pooled under the name
/// `alpha`, exit 0, no note (foreman-reproduced against pyarrow: reading
/// each file independently confirms `alpha == [1, 2]` and `omega == [500,
/// 600]`, two distinct columns, never one). This must now be refused, and
/// the error must name both actual column names, not print an
/// identical-looking pair of lists.
#[test]
fn describe_disjoint_column_names_same_type_is_refused_not_silently_merged() {
    let dir = TempDir::new().unwrap();
    let a = write_named_column(dir.path(), "x_alpha", "alpha", vec![1, 2]);
    let b = write_named_column(dir.path(), "x_omega", "omega", vec![500, 600]);

    let assert = pq()
        .args([
            "stats",
            "--describe",
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            "-f",
            "jsonl",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert!(
        !stderr.to_lowercase().contains("panic"),
        "must fail cleanly, not panic: {stderr}"
    );
    assert!(
        stderr.contains("alpha") && stderr.contains("omega"),
        "error must name the actual differing columns: {stderr}"
    );
}

/// Column names must be compared exactly, not case-insensitively: `Alpha`
/// and `alpha` are different columns, and silently unifying them would
/// reintroduce the same silent-merge bug under a different disguise.
#[test]
fn describe_column_name_case_difference_is_refused() {
    let dir = TempDir::new().unwrap();
    let a = write_named_column(dir.path(), "upper", "Alpha", vec![1, 2]);
    let b = write_named_column(dir.path(), "lower", "alpha", vec![3, 4]);

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
        stderr.contains("Alpha") && stderr.contains("alpha"),
        "case-differing names must be named in the error, not silently unified: {stderr}"
    );
}

/// One file has an extra column beyond the other's names — a degenerate
/// case of the name-set check, distinct from the wide/narrow panic-safety
/// tests above (which used positional column *count* mismatches with
/// overlapping names at each shared position).
#[test]
fn describe_extra_column_beyond_shared_names_is_refused() {
    let dir = TempDir::new().unwrap();
    let a = import_jsonl(dir.path(), "two_cols", "{\"x\": 1, \"y\": 2}\n");
    let b = import_jsonl(dir.path(), "one_col", "{\"x\": 1}\n");

    pq().args([
        "stats",
        "--describe",
        a.to_str().unwrap(),
        b.to_str().unwrap(),
    ])
    .assert()
    .failure();
}

/// Same column name, genuinely different type, must still be refused — the
/// name-set fix must not loosen the type check `describe_type_mismatch_*`
/// above already covers with `Timestamp` units; this pins the simpler
/// Int64-vs-Utf8 case under the same column name.
#[test]
fn describe_same_name_different_type_is_refused_control() {
    let dir = TempDir::new().unwrap();
    let a = import_jsonl(dir.path(), "int_id", "{\"id\": 1}\n");
    let b = import_jsonl(dir.path(), "string_id", "{\"id\": \"x\"}\n");

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
}

/// Field *metadata* differing across files (e.g. different writers stamping
/// their own provenance key) must not cause a rejection, mirroring the
/// nullability control above. `concat` never looks at metadata
/// (arrow-schema-53.4.1/src/field.rs:52-58 lists it as part of `Field::eq`,
/// but arrow-select-53.4.1/src/concat.rs:150-161 only ever compares bare
/// array `DataType`s), so a guard that rejects on metadata alone rejects
/// input `concat` handles fine.
#[test]
fn describe_differing_field_metadata_across_files_still_works() {
    let dir = TempDir::new().unwrap();

    let mut meta_a = HashMap::new();
    meta_a.insert("source".to_string(), "writer-a".to_string());
    let schema_a = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, true).with_metadata(meta_a)
    ]));
    let batch_a =
        RecordBatch::try_new(schema_a, vec![Arc::new(Int64Array::from(vec![1, 2]))]).unwrap();
    let a = write_batch(dir.path(), "meta_a", batch_a);

    let mut meta_b = HashMap::new();
    meta_b.insert("source".to_string(), "writer-b".to_string());
    meta_b.insert("extra".to_string(), "yes".to_string());
    let schema_b = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, true).with_metadata(meta_b)
    ]));
    let batch_b =
        RecordBatch::try_new(schema_b, vec![Arc::new(Int64Array::from(vec![3, 4]))]).unwrap();
    let b = write_batch(dir.path(), "meta_b", batch_b);

    pq().args([
        "stats",
        "--describe",
        a.to_str().unwrap(),
        b.to_str().unwrap(),
    ])
    .assert()
    .success();
}

/// The common real case: two files with the same columns in a different
/// order (`amount, price` vs `price, amount` — writers that don't agree on
/// column ordering). Refusing this would be needless; the underlying data
/// must also be physically realigned by name before concatenation, not just
/// waved through by the schema check, or this degenerates back into the
/// original bug with the roles of "name" and "position" swapped. If the fix
/// only relaxed the check without realigning `all_batches`, `amount` would
/// report `price`'s values here (max 4 instead of 40) and vice versa.
#[test]
fn describe_reordered_columns_same_names_are_unioned_correctly() {
    let dir = TempDir::new().unwrap();

    let schema_a = Arc::new(Schema::new(vec![
        Field::new("amount", DataType::Int64, true),
        Field::new("price", DataType::Float64, true),
    ]));
    let batch_a = RecordBatch::try_new(
        schema_a,
        vec![
            Arc::new(Int64Array::from(vec![10, 20])),
            Arc::new(Float64Array::from(vec![1.0, 2.0])),
        ],
    )
    .unwrap();
    let a = write_batch(dir.path(), "a", batch_a);

    let schema_b = Arc::new(Schema::new(vec![
        Field::new("price", DataType::Float64, true),
        Field::new("amount", DataType::Int64, true),
    ]));
    let batch_b = RecordBatch::try_new(
        schema_b,
        vec![
            Arc::new(Float64Array::from(vec![3.0, 4.0])),
            Arc::new(Int64Array::from(vec![30, 40])),
        ],
    )
    .unwrap();
    let b = write_batch(dir.path(), "b", batch_b);

    let json = describe_json(&[&a, &b], None);
    let cols = json["columns"].as_array().unwrap();
    let amount = cols
        .iter()
        .find(|c| c["column"] == "amount")
        .expect("amount column present in output");
    let price = cols
        .iter()
        .find(|c| c["column"] == "price")
        .expect("price column present in output");

    // Ground truth, hand-computed: amount = [10,20,30,40], price =
    // [1.0,2.0,3.0,4.0] — matching what `pyarrow.concat_tables` reports.
    assert_eq!(amount["count"], 4);
    assert_eq!(amount["min"], 10);
    assert_eq!(amount["max"], 40);
    assert_eq!(amount["mean"], 25.0);
    assert_eq!(price["count"], 4);
    assert_eq!(price["min"], 1.0);
    assert_eq!(price["max"], 4.0);
    assert_eq!(price["mean"], 2.5);
}

/// Duplicate column names *within a single file* are a distinct case from
/// cross-file name matching above — `describe` walks columns by index, not
/// by name, so two same-named columns must still each get their own row
/// with their own (not merged, not overwritten) statistics.
#[test]
fn describe_duplicate_column_names_within_one_file_yields_one_row_per_column() {
    let dir = TempDir::new().unwrap();
    let schema = Arc::new(Schema::new(vec![
        Field::new("x", DataType::Int64, true),
        Field::new("x", DataType::Int64, true),
    ]));
    let batch = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(Int64Array::from(vec![1, 2, 3])),
            Arc::new(Int64Array::from(vec![100, 200, 300])),
        ],
    )
    .unwrap();
    let f = write_batch(dir.path(), "dup", batch);

    let json = describe_json(&[&f], None);
    let cols = json["columns"].as_array().unwrap();
    assert_eq!(
        cols.len(),
        2,
        "duplicate-named columns must each get their own row: {json}"
    );
    assert_eq!(cols[0]["column"], "x");
    assert_eq!(cols[1]["column"], "x");
    assert_eq!(cols[0]["min"], 1, "first x column's own data: {json}");
    assert_eq!(cols[0]["max"], 3, "first x column's own data: {json}");
    assert_eq!(cols[1]["min"], 100, "second x column's own data: {json}");
    assert_eq!(cols[1]["max"], 300, "second x column's own data: {json}");
}

// ---------------------------------------------------------------------------
// `--sample-size` across multiple files.
//
// `pq stats --describe a.parquet b.parquet --sample-size N` used to read
// data with a shrinking `limit`, opening each file only as long as the
// budget wasn't yet exhausted, and `break`-ing the instant it hit zero. Two
// files holding [1,2,3] and [100,200,300] with `--sample-size 2` reported
// `count: 2, max: 2` — drawn entirely from the first file — with exit 0 and
// no indication `b.parquet` was never opened. Because the second file was
// never opened, its schema was never checked either, so a genuinely
// incompatible file could sit unread and unnoticed right where the sample
// happened to stop.
//
// Ground truth for every numeric assertion below (concatenated count, min,
// max, mean, stddev) is computed independently by hand from the literal
// input arrays, mirroring what `pyarrow.concat_tables` would report — never
// by comparing pq's output against another pq invocation.
// ---------------------------------------------------------------------------

/// Writes a single-column `Int64` file named `<name>.parquet` under `dir`.
fn write_ints(dir: &Path, name: &str, values: Vec<i64>) -> PathBuf {
    let schema = Arc::new(Schema::new(vec![Field::new("v", DataType::Int64, true)]));
    let batch = RecordBatch::try_new(schema, vec![Arc::new(Int64Array::from(values))]).unwrap();
    write_batch(dir, name, batch)
}

/// Runs `pq stats --describe <files...> [--sample-size N] -f json` and
/// parses stdout as JSON. A parse failure or non-success exit is itself a
/// test failure (never silently swallowed), so a broken harness cannot
/// masquerade as a passing assertion.
fn describe_json(files: &[&Path], sample_size: Option<usize>) -> serde_json::Value {
    let mut cmd = pq();
    cmd.arg("stats").arg("--describe");
    for f in files {
        cmd.arg(f);
    }
    if let Some(n) = sample_size {
        cmd.arg("--sample-size").arg(n.to_string());
    }
    cmd.arg("-f").arg("json");
    let assert = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("describe -f json produced invalid JSON: {e}\n{stdout}"))
}

fn file_entry<'a>(sampling: &'a serde_json::Value, path: &Path) -> &'a serde_json::Value {
    let path_str = path.to_str().unwrap();
    sampling["files"]
        .as_array()
        .expect("sampling.files must be an array")
        .iter()
        .find(|f| f["path"] == path_str)
        .unwrap_or_else(|| panic!("no sampling.files entry for {path_str}: {sampling}"))
}

/// `--sample-size` below the total row count, with the first file alone
/// already exceeding it: the classic repro. Values must be drawn only from
/// what was actually read (first 2 rows of `a`, per the concatenation-order
/// rule shared with `tail`/`sample`), `b` must appear in `sampling.files`
/// with `opened: false`, and `sampling.sampled` must be true.
#[test]
fn describe_sample_size_below_first_files_total_discloses_unread_file_in_json() {
    let dir = TempDir::new().unwrap();
    let a = write_ints(dir.path(), "a", vec![1, 2, 3]);
    let b = write_ints(dir.path(), "b", vec![100, 200, 300]);

    let json = describe_json(&[&a, &b], Some(2));

    // Ground truth: reading only the first 2 rows of the concatenation
    // (a's rows in order) gives count=2, min=1, max=2, mean=1.5,
    // stddev=0.5 (population stddev of [1,2]).
    let col = &json["columns"][0];
    assert_eq!(
        col["count"], 2,
        "must count only the rows actually read: {json}"
    );
    assert_eq!(col["min"], 1);
    assert_eq!(
        col["max"], 2,
        "max must come from the rows read, not the full dataset: {json}"
    );
    assert_eq!(col["mean"], 1.5);

    let sampling = &json["sampling"];
    assert_eq!(
        sampling["sampled"], true,
        "sampling.sampled must be true: {json}"
    );
    assert_eq!(sampling["sample_size"], 2);
    assert_eq!(sampling["rows_read"], 2);
    assert_eq!(
        sampling["rows_total"], 6,
        "must know the TRUE total across both files: {json}"
    );
    assert_eq!(sampling["files_total"], 2);
    assert_eq!(sampling["files_read"], 1);

    let a_entry = file_entry(sampling, &a);
    assert_eq!(a_entry["opened"], true);
    assert_eq!(a_entry["rows_read"], 2);
    assert_eq!(a_entry["rows_total"], 3);

    let b_entry = file_entry(sampling, &b);
    assert_eq!(
        b_entry["opened"], false,
        "b.parquet must be disclosed as NOT opened, not silently omitted: {json}"
    );
    assert_eq!(b_entry["rows_read"], 0);
    assert_eq!(
        b_entry["rows_total"], 3,
        "b's own total must still be known from its metadata: {json}"
    );
}

/// The `table` and `plain` renderers must also name the unread file, not
/// just the generic "sampled" note.
#[test]
fn describe_sample_size_below_total_names_unread_file_in_table_and_plain_notes() {
    let dir = TempDir::new().unwrap();
    let a = write_ints(dir.path(), "a", vec![1, 2, 3]);
    let b = write_ints(dir.path(), "b", vec![100, 200, 300]);

    for fmt in ["table", "plain"] {
        let assert = pq()
            .args([
                "stats",
                "--describe",
                a.to_str().unwrap(),
                b.to_str().unwrap(),
                "--sample-size",
                "2",
                "-f",
                fmt,
            ])
            .assert()
            .success();
        let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
        assert!(
            stdout.contains("b.parquet"),
            "-f {fmt} must name the unread file in its sampled note: {stdout}"
        );
    }
}

/// `--sample-size` exactly equal to the true total: every file must be
/// fully read and `sampling.sampled` must be false — the boundary must not
/// be misreported as a partial sample just because a cap was given.
#[test]
fn describe_sample_size_equal_to_total_reads_every_file_and_is_not_sampled() {
    let dir = TempDir::new().unwrap();
    let a = write_ints(dir.path(), "a", vec![1, 2, 3]);
    let b = write_ints(dir.path(), "b", vec![4, 5, 6]);

    let json = describe_json(&[&a, &b], Some(6));

    // Ground truth: concatenation of [1,2,3,4,5,6] has count=6, max=6.
    let col = &json["columns"][0];
    assert_eq!(col["count"], 6);
    assert_eq!(col["max"], 6);
    assert_eq!(col["min"], 1);

    let sampling = &json["sampling"];
    assert_eq!(
        sampling["sampled"], false,
        "sample_size == true total must not be reported as sampled: {json}"
    );
    assert_eq!(sampling["files_read"], 2);
    for f in sampling["files"].as_array().unwrap() {
        assert_eq!(
            f["opened"], true,
            "every file must be opened when the budget covers the full total: {json}"
        );
        assert_eq!(f["rows_read"], f["rows_total"]);
    }
}

/// `--sample-size` above the true total (the common case in practice, since
/// the default is 100000): behaviour must match the fully-unsampled
/// control exactly.
#[test]
fn describe_sample_size_above_total_matches_unsampled_control() {
    let dir = TempDir::new().unwrap();
    let a = write_ints(dir.path(), "a", vec![1, 2, 3]);
    let b = write_ints(dir.path(), "b", vec![4, 5, 6]);

    let sampled = describe_json(&[&a, &b], Some(1000));
    let unsampled = describe_json(&[&a, &b], None);

    assert_eq!(sampled["columns"], unsampled["columns"]);
    assert_eq!(sampled["sampling"]["sampled"], false);
    assert_eq!(unsampled["sampling"]["sampled"], false);
    assert_eq!(sampled["sampling"]["rows_read"], 6);
}

/// The dangerous half of the original bug: with a row budget that saturates
/// on the first file, a second file with a genuinely incompatible schema
/// used to be skipped along with its data, reaching exit 0. The schema
/// guard must now fire regardless of whether the row budget would ever
/// have reached that file.
#[test]
fn describe_sample_size_below_first_file_total_still_catches_incompatible_second_file() {
    let dir = TempDir::new().unwrap();
    let a = write_ints(dir.path(), "a", vec![1, 2, 3]);

    let string_schema = Arc::new(Schema::new(vec![Field::new("v", DataType::Utf8, true)]));
    let string_batch = RecordBatch::try_new(
        string_schema,
        vec![Arc::new(StringArray::from(vec!["x", "y", "z"]))],
    )
    .unwrap();
    let b = write_batch(dir.path(), "b", string_batch);

    // --sample-size 1 is smaller than a's own row count, so the pre-fix
    // code would never have opened b.parquet at all.
    let assert = pq()
        .args([
            "stats",
            "--describe",
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            "--sample-size",
            "1",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert!(
        stderr.contains("different schemas"),
        "an incompatible file behind a saturated sample budget must still \
         be rejected, not silently skipped: {stderr}"
    );
    assert!(
        stderr.contains("a.parquet") && stderr.contains("b.parquet"),
        "error must name both files: {stderr}"
    );
}

/// A zero-row file sitting between two non-empty files must not be
/// misreported as "not read" (it WAS opened; it simply holds nothing) and
/// must not break the row accounting for the files around it.
#[test]
fn describe_zero_row_file_among_nonempty_files_is_opened_and_accounted() {
    let dir = TempDir::new().unwrap();
    let a = write_ints(dir.path(), "a", vec![1, 2, 3]);
    let empty = write_ints(dir.path(), "empty", vec![]);
    let b = write_ints(dir.path(), "b", vec![10, 20]);

    // Sample size covers the whole dataset (5 rows) so every file is
    // expected to be opened.
    let json = describe_json(&[&a, &empty, &b], Some(100));

    let col = &json["columns"][0];
    assert_eq!(
        col["count"], 5,
        "the empty file must not distort the total row count: {json}"
    );
    assert_eq!(col["max"], 20);
    assert_eq!(col["min"], 1);

    let sampling = &json["sampling"];
    assert_eq!(sampling["files_total"], 3);
    assert_eq!(
        sampling["files_read"], 3,
        "the empty file counts as opened, not skipped: {json}"
    );

    let empty_entry = file_entry(sampling, &empty);
    assert_eq!(
        empty_entry["opened"], true,
        "an empty file that was genuinely opened must not be reported the \
         same way as a file the budget skipped: {json}"
    );
    assert_eq!(empty_entry["rows_read"], 0);
    assert_eq!(empty_entry["rows_total"], 0);
}

/// Control: a single file must behave exactly as before the fix, modulo the
/// new `sampling` envelope in the JSON output. This guards against the
/// metadata-first-pass restructuring changing single-file results.
#[test]
fn describe_single_file_is_unaffected_control() {
    let dir = TempDir::new().unwrap();
    let a = write_ints(dir.path(), "a", vec![1, 2, 3, 4, 5]);

    let json = describe_json(&[&a], None);
    let col = &json["columns"][0];
    assert_eq!(col["count"], 5);
    assert_eq!(col["min"], 1);
    assert_eq!(col["max"], 5);
    assert_eq!(col["mean"], 3.0);

    let sampling = &json["sampling"];
    assert_eq!(sampling["sampled"], false);
    assert_eq!(sampling["files_total"], 1);
    assert_eq!(sampling["files_read"], 1);
    assert_eq!(sampling["rows_read"], 5);
    assert_eq!(sampling["rows_total"], 5);

    // A sample size smaller than the single file's own row count must
    // still correctly mark it sampled (single-file sampling already worked
    // before this fix; this is the non-regression check).
    let sampled = describe_json(&[&a], Some(2));
    assert_eq!(sampled["sampling"]["sampled"], true);
    assert_eq!(sampled["columns"][0]["count"], 2);
    assert_eq!(sampled["columns"][0]["max"], 2);
}

/// Plain `stats` (no `--describe`) is dispatched once per resolved file
/// (`main.rs`'s `for f in &resolved { commands::stats::run(f, format)?; }`)
/// rather than being pooled and sampled like `--describe`. It takes no
/// `--sample-size` flag at all, so every named file's own stats are always
/// printed — there is no budget to silently exhaust. This test exists to
/// pin that shape down: if `stats` (no `--describe`) is ever changed to
/// pool multiple files, this must be revisited for the same bug.
#[test]
fn plain_stats_prints_every_file_independently_no_shared_bug() {
    let dir = TempDir::new().unwrap();
    let a = write_ints(dir.path(), "a", vec![1, 2, 3]);
    let b = write_ints(dir.path(), "b", vec![100, 200, 300]);

    let assert = pq()
        .args([
            "stats",
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            "-f",
            "jsonl",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    // `-f jsonl` prints one compact line per file (see main.rs's per-file
    // loop: `for f in &resolved { commands::stats::run(f, format)?; }`).
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        lines.len(),
        2,
        "expected one stats block per file: {stdout}"
    );
    assert!(
        lines[0].contains("\"min_value\":\"1\""),
        "first block must be a's stats: {stdout}"
    );
    assert!(
        lines[1].contains("\"min_value\":\"100\""),
        "second block must be b's stats: {stdout}"
    );
}
