//! Guards for a silent wrong-data bug in `pq cat`'s **default** output
//! format: `render_table` (and its sibling `render_plain`, `-f plain`) built
//! their header from `batches[0]`'s schema and then zipped every later
//! batch's values into that header *positionally*. When a later file has the
//! same column names in a different order -- or a different column set
//! entirely -- values landed under the wrong-named header, silently, exit 0.
//!
//! `-f csv` had exactly this bug and was fixed to build a union header and
//! resolve each batch against it by `(name, occurrence)`
//! (`write_output::union_columns` / `column_indices`; see
//! `csv_correctness_tests.rs`). `-f table` and `-f plain` still built the
//! header from `batches[0]` alone and zipped positionally -- this file is the
//! same guard, for those two renderers.
//!
//! Instrument: values are asserted against literals that were independently
//! checked against `pyarrow` during development --
//! `pyarrow.parquet.ParquetFile(path).read()` on each fixture, read
//! column-by-column (`table.column(i).to_pylist()`, which -- unlike
//! `to_pylist()` on the whole table -- does not collapse duplicate-named
//! columns through a Python dict). `pq` is never used to validate `pq`.

use arrow::array::{ArrayRef, Int64Array, RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;

fn pq() -> Command {
    assert_cmd::cargo_bin_cmd!("pq")
}

/// Write a single-row-batch-friendly parquet file with the given
/// `(name, values)` columns, in exactly the order given -- this is what lets
/// tests build "same names, different order" fixtures without relying on any
/// pq-side schema inference to preserve order.
fn write_str_parquet(dir: &Path, filename: &str, columns: &[(&str, &[&str])]) -> PathBuf {
    let fields: Vec<Field> = columns
        .iter()
        .map(|(name, _)| Field::new(*name, DataType::Utf8, false))
        .collect();
    let schema = Arc::new(Schema::new(fields));
    let arrays: Vec<ArrayRef> = columns
        .iter()
        .map(|(_, values)| Arc::new(StringArray::from(values.to_vec())) as ArrayRef)
        .collect();
    let batch = RecordBatch::try_new(schema.clone(), arrays).unwrap();

    let out = dir.join(filename);
    let file = fs::File::create(&out).unwrap();
    let mut writer = parquet::arrow::ArrowWriter::try_new(file, schema, None).unwrap();
    writer.write(&batch).unwrap();
    writer.close().unwrap();
    out
}

/// A parquet file with two int64 columns both named `id`, holding
/// distinguishable data (1,2 in the first occurrence; 10,20 in the second).
/// Not reachable through `pq import` -- a JSON object cannot have duplicate
/// keys -- so built directly, same fixture shape as
/// `csv_correctness_tests::dup_column_parquet`.
fn dup_id_parquet(dir: &Path, filename: &str) -> PathBuf {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("id", DataType::Int64, false),
    ]));
    let first: ArrayRef = Arc::new(Int64Array::from(vec![1_i64, 2]));
    let second: ArrayRef = Arc::new(Int64Array::from(vec![10_i64, 20]));
    let batch = RecordBatch::try_new(schema.clone(), vec![first, second]).unwrap();

    let out = dir.join(filename);
    let file = fs::File::create(&out).unwrap();
    let mut writer = parquet::arrow::ArrowWriter::try_new(file, schema, None).unwrap();
    writer.write(&batch).unwrap();
    writer.close().unwrap();
    out
}

/// Parse `-f table`'s comfy-table output into (header, rows) of trimmed
/// cells. comfy-table's UTF8_FULL preset separates the outer border with
/// `│` and columns *within* a row with `┆` -- confirmed by inspecting real
/// output, not assumed.
fn parse_table(text: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let cell_rows: Vec<Vec<String>> = text
        .lines()
        .filter(|line| line.starts_with('│'))
        .map(|line| {
            line.trim_matches('│')
                .split('┆')
                .map(|c| c.trim().to_string())
                .collect()
        })
        .collect();
    assert!(
        !cell_rows.is_empty(),
        "no table rows found -- the renderer never ran or the format changed: {text}"
    );
    let header = cell_rows[0].clone();
    let rows = cell_rows[1..].to_vec();
    (header, rows)
}

/// Parse `-f plain`'s tab-separated output into (header, rows).
fn parse_plain(text: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let mut lines = text.lines();
    let header: Vec<String> = lines
        .next()
        .unwrap_or_else(|| panic!("no plain output at all: {text}"))
        .split('\t')
        .map(|s| s.to_string())
        .collect();
    let rows: Vec<Vec<String>> = lines
        .map(|line| line.split('\t').map(|s| s.to_string()).collect())
        .collect();
    (header, rows)
}

/// Look up a row's value by column name in a (possibly reordered) header.
fn cell<'a>(header: &[String], row: &'a [String], name: &str) -> &'a str {
    let idx = header
        .iter()
        .position(|h| h == name)
        .unwrap_or_else(|| panic!("column '{name}' not in header {header:?}"));
    &row[idx]
}

// ---------------------------------------------------------------------------
// Same names, reordered
// ---------------------------------------------------------------------------

#[test]
fn table_aligns_reordered_columns_by_name() {
    let dir = TempDir::new().unwrap();
    let o1 = write_str_parquet(dir.path(), "o1.parquet", &[("a", &["A1"]), ("b", &["B1"])]);
    // Same names, reversed schema order.
    let o2 = write_str_parquet(dir.path(), "o2.parquet", &[("b", &["B2"]), ("a", &["A2"])]);

    let out = pq()
        .args([
            "cat",
            o1.to_str().unwrap(),
            o2.to_str().unwrap(),
            "-f",
            "table",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&out).to_string();
    let (header, rows) = parse_table(&text);

    assert_eq!(rows.len(), 2, "expected 2 data rows: {rows:?}");
    // pyarrow ground truth: o1 is a=A1,b=B1; o2 is a=A2,b=B2 (reordered
    // schema, unchanged logical rows).
    assert_eq!(cell(&header, &rows[0], "a"), "A1");
    assert_eq!(cell(&header, &rows[0], "b"), "B1");
    assert_eq!(cell(&header, &rows[1], "a"), "A2");
    assert_eq!(
        cell(&header, &rows[1], "b"),
        "B2",
        "row from o2 (schema order b,a) must show b's own value under 'b', \
         not a's value shifted in from the positional-zip bug: {text}"
    );
}

#[test]
fn plain_aligns_reordered_columns_by_name() {
    let dir = TempDir::new().unwrap();
    let o1 = write_str_parquet(dir.path(), "o1.parquet", &[("a", &["A1"]), ("b", &["B1"])]);
    let o2 = write_str_parquet(dir.path(), "o2.parquet", &[("b", &["B2"]), ("a", &["A2"])]);

    let out = pq()
        .args([
            "cat",
            o1.to_str().unwrap(),
            o2.to_str().unwrap(),
            "-f",
            "plain",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&out).to_string();
    let (header, rows) = parse_plain(&text);

    assert_eq!(rows.len(), 2, "expected 2 data rows: {rows:?}");
    assert_eq!(cell(&header, &rows[0], "a"), "A1");
    assert_eq!(cell(&header, &rows[0], "b"), "B1");
    assert_eq!(cell(&header, &rows[1], "a"), "A2");
    assert_eq!(cell(&header, &rows[1], "b"), "B2");
}

// ---------------------------------------------------------------------------
// Disjoint column sets
// ---------------------------------------------------------------------------

#[test]
fn table_header_is_union_for_disjoint_column_sets() {
    let dir = TempDir::new().unwrap();
    let f1 = write_str_parquet(dir.path(), "f1.parquet", &[("a", &["A1"])]);
    let f2 = write_str_parquet(dir.path(), "f2.parquet", &[("c", &["C2"])]);

    let out = pq()
        .args([
            "cat",
            f1.to_str().unwrap(),
            f2.to_str().unwrap(),
            "-f",
            "table",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&out).to_string();
    let (header, rows) = parse_table(&text);

    assert!(
        header.contains(&"a".to_string()),
        "header missing 'a': {header:?}"
    );
    assert!(
        header.contains(&"c".to_string()),
        "header missing 'c' -- the second file's only column was dropped: {header:?}"
    );
    assert_eq!(rows.len(), 2);
    assert_eq!(cell(&header, &rows[0], "a"), "A1");
    assert_eq!(cell(&header, &rows[1], "c"), "C2");
}

// ---------------------------------------------------------------------------
// Partial overlap, three files
// ---------------------------------------------------------------------------

#[test]
fn table_aligns_three_files_with_partial_overlap() {
    let dir = TempDir::new().unwrap();
    let p1 = write_str_parquet(dir.path(), "p1.parquet", &[("a", &["A3"]), ("b", &["B3"])]);
    let p2 = write_str_parquet(dir.path(), "p2.parquet", &[("b", &["B4"]), ("c", &["C4"])]);
    let p3 = write_str_parquet(dir.path(), "p3.parquet", &[("c", &["C5"]), ("a", &["A5"])]);

    let out = pq()
        .args([
            "cat",
            p1.to_str().unwrap(),
            p2.to_str().unwrap(),
            p3.to_str().unwrap(),
            "-f",
            "table",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&out).to_string();
    let (header, rows) = parse_table(&text);

    assert_eq!(rows.len(), 3, "expected 3 data rows: {rows:?}");
    // pyarrow ground truth: p1={a:A3,b:B3}, p2={b:B4,c:C4}, p3={c:C5,a:A5}.
    assert_eq!(cell(&header, &rows[0], "a"), "A3");
    assert_eq!(cell(&header, &rows[0], "b"), "B3");
    assert_eq!(cell(&header, &rows[1], "b"), "B4");
    assert_eq!(cell(&header, &rows[1], "c"), "C4");
    assert_eq!(cell(&header, &rows[2], "c"), "C5");
    assert_eq!(cell(&header, &rows[2], "a"), "A5");
}

// ---------------------------------------------------------------------------
// Missing-column cell vs genuine NULL must be distinguishable
// ---------------------------------------------------------------------------

#[test]
fn table_missing_column_cell_differs_from_blank_null_cell() {
    let dir = TempDir::new().unwrap();
    // File 1 has both columns, with a genuine NULL in `b`.
    let schema = Arc::new(Schema::new(vec![
        Field::new("a", DataType::Utf8, false),
        Field::new("b", DataType::Utf8, true),
    ]));
    let a_arr: ArrayRef = Arc::new(StringArray::from(vec!["A1"]));
    let b_arr: ArrayRef = Arc::new(StringArray::from(vec![None::<&str>]));
    let batch = RecordBatch::try_new(schema.clone(), vec![a_arr, b_arr]).unwrap();
    let f1 = dir.path().join("f1.parquet");
    let file = fs::File::create(&f1).unwrap();
    let mut writer = parquet::arrow::ArrowWriter::try_new(file, schema, None).unwrap();
    writer.write(&batch).unwrap();
    writer.close().unwrap();

    // File 2 doesn't have `b` at all.
    let f2 = write_str_parquet(dir.path(), "f2.parquet", &[("a", &["A2"])]);

    let out = pq()
        .args([
            "cat",
            f1.to_str().unwrap(),
            f2.to_str().unwrap(),
            "-f",
            "table",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&out).to_string();
    let (header, rows) = parse_table(&text);

    let null_cell = cell(&header, &rows[0], "b");
    let missing_cell = cell(&header, &rows[1], "b");
    assert_eq!(
        null_cell, "",
        "a genuine NULL should render blank: {rows:?}"
    );
    assert_ne!(
        missing_cell, "",
        "a column the file doesn't have at all must not render as an empty \
         string indistinguishable from NULL: {rows:?}"
    );
    assert_ne!(
        null_cell, missing_cell,
        "NULL and 'column absent from this file' must be visually distinguishable: {rows:?}"
    );
}

// ---------------------------------------------------------------------------
// Duplicate column names must keep working (no regression from this fix)
// ---------------------------------------------------------------------------

#[test]
fn table_duplicate_named_columns_still_render_correctly() {
    let dir = TempDir::new().unwrap();
    let f = dup_id_parquet(dir.path(), "dup.parquet");

    let out = pq()
        .args(["cat", f.to_str().unwrap(), "-f", "table"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&out).to_string();
    let (header, rows) = parse_table(&text);

    assert_eq!(
        header,
        vec!["id".to_string(), "id".to_string()],
        "duplicate-named column dropped from table header: {text}"
    );
    assert_eq!(
        rows,
        vec![
            vec!["1".to_string(), "10".to_string()],
            vec!["2".to_string(), "20".to_string()],
        ],
        "duplicate-named columns not resolved positionally: {text}"
    );
}

/// A file with duplicate names, alongside a file without, combined -- the
/// dup-name occurrence resolution must still work per-schema when unioned
/// against a schema that has no duplicates at all.
#[test]
fn table_duplicate_named_file_combined_with_plain_file() {
    let dir = TempDir::new().unwrap();
    let dup = dup_id_parquet(dir.path(), "dup.parquet");
    // A second file with a single `id` column plus an unrelated one.
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("tag", DataType::Utf8, false),
    ]));
    let id_arr: ArrayRef = Arc::new(Int64Array::from(vec![99_i64]));
    let tag_arr: ArrayRef = Arc::new(StringArray::from(vec!["x"]));
    let batch = RecordBatch::try_new(schema.clone(), vec![id_arr, tag_arr]).unwrap();
    let plain = dir.path().join("plain.parquet");
    let file = fs::File::create(&plain).unwrap();
    let mut writer = parquet::arrow::ArrowWriter::try_new(file, schema, None).unwrap();
    writer.write(&batch).unwrap();
    writer.close().unwrap();

    let out = pq()
        .args([
            "cat",
            dup.to_str().unwrap(),
            plain.to_str().unwrap(),
            "-f",
            "table",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&out).to_string();
    let (header, rows) = parse_table(&text);

    // Union: id(occurrence 0), id(occurrence 1), tag. The plain file has no
    // second `id` occurrence and no `id` at occurrence 1, so its first row
    // must show its single id (99) under the *first* id column, the second
    // id column marked as this file's missing column, and its own tag value.
    assert_eq!(
        header,
        vec!["id".to_string(), "id".to_string(), "tag".to_string()]
    );
    assert_eq!(rows[0][0], "1", "dup file's id-occurrence-0: {rows:?}");
    assert_eq!(rows[0][1], "10", "dup file's id-occurrence-1: {rows:?}");
    assert_eq!(rows[1][0], "2");
    assert_eq!(rows[1][1], "20");
    // `dup.parquet` has no `tag` column at all -- distinct from a NULL, so
    // it must show the missing-column marker, not an empty string.
    let missing = cell(&header, &rows[0], "tag");
    assert_ne!(
        missing, "",
        "missing column rendered as blank, indistinguishable from NULL: {rows:?}"
    );
    assert_eq!(
        cell(&header, &rows[0], "tag"),
        cell(&header, &rows[1], "tag")
    );
    assert_eq!(cell(&header, &rows[2], "tag"), "x");
    // The plain file's single `id` (99) must land under the first `id`
    // column (occurrence 0), never silently dropped or duplicated.
    assert_eq!(rows[2][0], "99");
    // ...and it has no second `id` occurrence, so that cell is missing too.
    assert_eq!(
        rows[2][1], missing,
        "plain file's absent id-occurrence-1: {rows:?}"
    );
}

// ---------------------------------------------------------------------------
// Single file is an unaffected control
// ---------------------------------------------------------------------------

#[test]
fn table_single_file_output_is_unchanged_by_the_fix() {
    let dir = TempDir::new().unwrap();
    let f = write_str_parquet(dir.path(), "f.parquet", &[("a", &["A1"]), ("b", &["B1"])]);

    let out = pq()
        .args(["cat", f.to_str().unwrap(), "-f", "table"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&out).to_string();
    let (header, rows) = parse_table(&text);

    assert_eq!(header, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(rows, vec![vec!["A1".to_string(), "B1".to_string()]]);
}

// ---------------------------------------------------------------------------
// Cross-format agreement: the strongest guard for this class of bug.
// -f table, -f csv, -f jsonl must report the same logical row set for the
// same input, regardless of which renderer disagreed before the fix.
// ---------------------------------------------------------------------------

#[test]
fn table_csv_and_jsonl_agree_on_reordered_columns() {
    let dir = TempDir::new().unwrap();
    let o1 = write_str_parquet(dir.path(), "o1.parquet", &[("a", &["A1"]), ("b", &["B1"])]);
    let o2 = write_str_parquet(dir.path(), "o2.parquet", &[("b", &["B2"]), ("a", &["A2"])]);

    let run = |fmt: &str| -> String {
        let out = pq()
            .args(["cat", o1.to_str().unwrap(), o2.to_str().unwrap(), "-f", fmt])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        String::from_utf8_lossy(&out).to_string()
    };

    let table_text = run("table");
    let csv_text = run("csv");
    let jsonl_text = run("jsonl");

    let (t_header, t_rows) = parse_table(&table_text);

    let mut csv_rdr = csv::ReaderBuilder::new().from_reader(csv_text.as_bytes());
    let c_header: Vec<String> = csv_rdr
        .headers()
        .unwrap()
        .iter()
        .map(String::from)
        .collect();
    let c_rows: Vec<Vec<String>> = csv_rdr
        .records()
        .map(|r| r.unwrap().iter().map(String::from).collect())
        .collect();

    let j_rows: Vec<serde_json::Value> = jsonl_text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();

    assert_eq!(t_rows.len(), 2);
    assert_eq!(c_rows.len(), 2);
    assert_eq!(j_rows.len(), 2);

    for (i, want) in [("A1", "B1"), ("A2", "B2")].into_iter().enumerate() {
        let (want_a, want_b) = want;
        assert_eq!(
            cell(&t_header, &t_rows[i], "a"),
            want_a,
            "table row {i}, column a"
        );
        assert_eq!(
            cell(&t_header, &t_rows[i], "b"),
            want_b,
            "table row {i}, column b"
        );
        assert_eq!(
            cell(&c_header, &c_rows[i], "a"),
            want_a,
            "csv row {i}, column a"
        );
        assert_eq!(
            cell(&c_header, &c_rows[i], "b"),
            want_b,
            "csv row {i}, column b"
        );
        assert_eq!(j_rows[i]["a"], want_a, "jsonl row {i}, column a");
        assert_eq!(j_rows[i]["b"], want_b, "jsonl row {i}, column b");
    }
}
