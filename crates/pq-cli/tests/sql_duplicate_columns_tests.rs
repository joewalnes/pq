//! Guards for: `pq sql` silently dropped duplicate-named columns.
//!
//! Parquet legally permits two top-level columns with the same name. Every
//! non-DataFusion path in pq (`cat`, `export`) carries both through. The
//! DataFusion paths (`pq sql`, and `pq cat --where`, which is also planned by
//! DataFusion) used to lose one, silently, exit 0.
//!
//! Where the loss happened, measured (see DIARY.md for the probe output):
//! `SessionContext::register_parquet` builds a `ListingTable` whose schema is
//! inferred by merging the file's arrow schema, and that merge collapses
//! same-named fields. The `TableProvider` pq hands DataFusion therefore
//! already has one `id` where the file has two — the loss is upstream of
//! planning, upstream of projection, and upstream of every writer.
//!
//! The fix disambiguates *visibly*: a file with duplicate column names is
//! registered under unique names (`id`, `id_1`, ...) with a note on stderr.
//! These guards assert the class — every duplicate arity, explicit and
//! implicit projection, both DataFusion entry points — and keep `cat`/
//! `export` as controls so a regression on the paths that already worked is
//! caught here too.
//!
//! Assertions are on emitted bytes, never on exit codes: the exit code was 0
//! before the fix and is 0 after.

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn pq() -> Command {
    assert_cmd::cargo_bin_cmd!("pq")
}

/// Write a parquet file whose top-level schema has exactly the given column
/// names, in order, each an Int64 column holding `[base, base + 1]` where
/// `base` is `(index + 1) * 10^index`-ish — in practice each column gets its
/// own distinguishable pair so a positional mix-up is visible.
///
/// Built with arrow/parquet directly rather than through `pq import`,
/// because no import path can *produce* duplicate names — that is the whole
/// point of the fixture.
fn dup_parquet(dir: &Path, name: &str, columns: &[&str]) -> PathBuf {
    write_parquet(dir, name, columns, false)
}

/// Same fixture with *nullable* columns. Needed wherever DataFusion has to
/// union files with different schemas: the gaps it fills are nulls, and a
/// non-nullable column rejects them ("Column 'c' is declared as non-nullable
/// but contains null values") — a property of the fixture, not of anything
/// under test here.
fn nullable_parquet(dir: &Path, name: &str, columns: &[&str]) -> PathBuf {
    write_parquet(dir, name, columns, true)
}

fn write_parquet(dir: &Path, name: &str, columns: &[&str], nullable: bool) -> PathBuf {
    use arrow::array::{ArrayRef, Int64Array, RecordBatch};
    use arrow::datatypes::{DataType, Field, Schema};
    use std::sync::Arc;

    let schema = Arc::new(Schema::new(
        columns
            .iter()
            .map(|c| Field::new(*c, DataType::Int64, nullable))
            .collect::<Vec<_>>(),
    ));
    let arrays: Vec<ArrayRef> = (0..columns.len())
        .map(|i| {
            let base = (i as i64 + 1) * 100;
            Arc::new(Int64Array::from(vec![base + 1, base + 2])) as ArrayRef
        })
        .collect();
    let batch = RecordBatch::try_new(schema.clone(), arrays).unwrap();

    let out = dir.join(format!("{name}.parquet"));
    let file = fs::File::create(&out).unwrap();
    let mut writer = parquet::arrow::ArrowWriter::try_new(file, schema, None).unwrap();
    writer.write(&batch).unwrap();
    writer.close().unwrap();
    out
}

/// The values `dup_parquet` puts in column `i`, as strings.
fn expected_col(i: usize) -> Vec<String> {
    let base = (i as i64 + 1) * 100;
    vec![(base + 1).to_string(), (base + 2).to_string()]
}

/// Parse CSV strictly — a ragged record is an error, not a silent accept.
fn parse_csv(bytes: &[u8]) -> (Vec<String>, Vec<Vec<String>>) {
    let mut rdr = csv::ReaderBuilder::new().from_reader(bytes);
    let header: Vec<String> = rdr
        .headers()
        .unwrap_or_else(|e| {
            panic!(
                "not valid CSV: {e}\nraw: {:?}",
                String::from_utf8_lossy(bytes)
            )
        })
        .iter()
        .map(|s| s.to_string())
        .collect();
    let rows: Vec<Vec<String>> = rdr
        .records()
        .map(|r| {
            r.unwrap_or_else(|e| {
                panic!(
                    "ragged CSV record: {e}\nraw: {:?}",
                    String::from_utf8_lossy(bytes)
                )
            })
            .iter()
            .map(|s| s.to_string())
            .collect()
        })
        .collect();
    (header, rows)
}

/// Run `pq` and return (stdout, stderr), asserting success. Both streams are
/// returned because the diagnostic that makes the rename non-silent lives on
/// stderr and a check that ignored it would ratify a silent fix.
fn run_ok(args: &[&str]) -> (Vec<u8>, String) {
    let out = pq().args(args).assert().success().get_output().clone();
    (out.stdout, String::from_utf8_lossy(&out.stderr).to_string())
}

/// Every column's data must be present exactly once, in schema order, under
/// *some* unique header name. Names are asserted separately where the
/// specific name is the contract; here the contract is "no column was lost
/// and none was duplicated in place of another".
fn assert_all_columns_present(csv_bytes: &[u8], n: usize, case: &str) {
    let (header, rows) = parse_csv(csv_bytes);
    assert_eq!(
        header.len(),
        n,
        "[{case}] expected {n} columns, got {header:?} (raw: {:?})",
        String::from_utf8_lossy(csv_bytes)
    );
    let mut sorted = header.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        n,
        "[{case}] header names are not unique — a SQL result set with \
         duplicate names is unreferenceable: {header:?}"
    );
    assert_eq!(rows.len(), 2, "[{case}] expected 2 data rows: {rows:?}");
    for i in 0..n {
        let want = expected_col(i);
        let got: Vec<String> = rows.iter().map(|r| r[i].clone()).collect();
        assert_eq!(
            got,
            want,
            "[{case}] column {i} (header {:?}) lost or shifted its data \
             (raw: {:?})",
            header[i],
            String::from_utf8_lossy(csv_bytes)
        );
    }
}

// ---------------------------------------------------------------------------
// The core class: `pq sql` must not lose a duplicate-named column
// ---------------------------------------------------------------------------

#[test]
fn sql_select_star_keeps_both_duplicate_columns() {
    let dir = TempDir::new().unwrap();
    let f = dup_parquet(dir.path(), "dup2", &["id", "id"]);

    // Reference instrument: `cat -f csv` shares no code with the DataFusion
    // path and keeps both columns today. If this half fails, the fixture or
    // the CSV writer is at fault, not `sql`.
    let (cat_out, _) = run_ok(&["cat", f.to_str().unwrap(), "-f", "csv"]);
    let (cat_header, cat_rows) = parse_csv(&cat_out);
    assert_eq!(
        cat_header,
        vec!["id", "id"],
        "reference instrument (`cat -f csv`) does not show two `id` columns; \
         the fixture is broken, not `sql`"
    );
    assert_eq!(cat_rows.len(), 2);

    let (sql_out, _) = run_ok(&[
        "sql",
        &format!("SELECT * FROM '{}'", f.to_str().unwrap()),
        "-f",
        "csv",
    ]);
    assert_all_columns_present(&sql_out, 2, "sql SELECT * -f csv");
}

#[test]
fn sql_select_star_keeps_three_duplicate_columns() {
    let dir = TempDir::new().unwrap();
    let f = dup_parquet(dir.path(), "dup3", &["id", "id", "id"]);
    let (out, _) = run_ok(&[
        "sql",
        &format!("SELECT * FROM '{}'", f.to_str().unwrap()),
        "-f",
        "csv",
    ]);
    assert_all_columns_present(&out, 3, "sql SELECT * (3 duplicates)");
}

#[test]
fn sql_keeps_duplicates_mixed_with_unique_columns() {
    let dir = TempDir::new().unwrap();
    let f = dup_parquet(dir.path(), "mixed", &["a", "id", "b", "id"]);
    let (out, _) = run_ok(&[
        "sql",
        &format!("SELECT * FROM '{}'", f.to_str().unwrap()),
        "-f",
        "csv",
    ]);
    assert_all_columns_present(&out, 4, "sql SELECT * (duplicates among uniques)");
    let (header, _) = parse_csv(&out);
    assert_eq!(
        &header[0], "a",
        "a non-duplicated column must keep its own name: {header:?}"
    );
    assert_eq!(
        &header[2], "b",
        "a non-duplicated column must keep its own name: {header:?}"
    );
    assert_eq!(
        &header[1], "id",
        "the first occurrence keeps the original name: {header:?}"
    );
}

/// The renamed column must be *referenceable*. This is the difference
/// between a fix and a cosmetic change: before the fix there was no query
/// at all that could return the second `id`.
#[test]
fn sql_explicit_projection_can_name_the_renamed_column() {
    let dir = TempDir::new().unwrap();
    let f = dup_parquet(dir.path(), "dup2", &["id", "id"]);

    // Discover the assigned name from SELECT * rather than hard-coding it in
    // every test, but assert the documented form here so the naming scheme
    // itself is pinned somewhere.
    let (star, _) = run_ok(&[
        "sql",
        &format!("SELECT * FROM '{}'", f.to_str().unwrap()),
        "-f",
        "csv",
    ]);
    let (header, _) = parse_csv(&star);
    assert_eq!(
        header,
        vec!["id", "id_1"],
        "documented disambiguation scheme changed"
    );

    let (out, _) = run_ok(&[
        "sql",
        &format!("SELECT id_1 FROM '{}'", f.to_str().unwrap()),
        "-f",
        "csv",
    ]);
    let (h, rows) = parse_csv(&out);
    assert_eq!(h, vec!["id_1"]);
    assert_eq!(
        rows.iter().map(|r| r[0].clone()).collect::<Vec<_>>(),
        expected_col(1),
        "`SELECT id_1` returned the wrong column's data: {rows:?}"
    );
}

/// The unrenamed name must still resolve to the *first* column, not to
/// whichever one the reader happened to bind last. Before the fix,
/// `SELECT id` returned the SECOND column's values (10, 20 in the original
/// report) — so this asserts a correction, not merely a survival.
#[test]
fn sql_bare_name_resolves_to_the_first_duplicate() {
    let dir = TempDir::new().unwrap();
    let f = dup_parquet(dir.path(), "dup2", &["id", "id"]);
    let (out, _) = run_ok(&[
        "sql",
        &format!("SELECT id FROM '{}'", f.to_str().unwrap()),
        "-f",
        "csv",
    ]);
    let (h, rows) = parse_csv(&out);
    assert_eq!(h, vec!["id"]);
    assert_eq!(
        rows.iter().map(|r| r[0].clone()).collect::<Vec<_>>(),
        expected_col(0),
        "`SELECT id` must return the first `id` column, not a later one: {rows:?}"
    );
}

/// Renaming must not silently collide with a column that already has the
/// generated name — that would trade one silent loss for another.
#[test]
fn rename_does_not_collide_with_an_existing_column() {
    let dir = TempDir::new().unwrap();
    let f = dup_parquet(dir.path(), "collide", &["id", "id", "id_1"]);
    let (out, _) = run_ok(&[
        "sql",
        &format!("SELECT * FROM '{}'", f.to_str().unwrap()),
        "-f",
        "csv",
    ]);
    assert_all_columns_present(&out, 3, "sql SELECT * (rename collides with real id_1)");
    let (header, _) = parse_csv(&out);
    assert_eq!(
        header[0], "id",
        "first occurrence keeps its name: {header:?}"
    );
    assert_eq!(
        header[2], "id_1",
        "a genuine pre-existing `id_1` must keep its own name: {header:?}"
    );
    assert_ne!(
        header[1], "id_1",
        "the renamed duplicate stole a real column's name: {header:?}"
    );
}

/// Names differing only in case are distinct Parquet columns. Whatever
/// DataFusion does with them, both columns' data must reach the output.
#[test]
fn sql_keeps_columns_differing_only_in_case() {
    let dir = TempDir::new().unwrap();
    let f = dup_parquet(dir.path(), "case", &["id", "ID"]);
    let (out, _) = run_ok(&[
        "sql",
        &format!("SELECT * FROM '{}'", f.to_str().unwrap()),
        "-f",
        "csv",
    ]);
    assert_all_columns_present(&out, 2, "sql SELECT * (case-differing names)");
}

/// `pq cat --where` is planned by DataFusion too (`query_with_where`), so it
/// carried the identical defect. Fixing only `pq sql` would leave it.
#[test]
fn cat_where_keeps_both_duplicate_columns() {
    let dir = TempDir::new().unwrap();
    let f = dup_parquet(dir.path(), "dup2", &["id", "id"]);
    let (out, _) = run_ok(&["cat", f.to_str().unwrap(), "--where", "1 = 1", "-f", "csv"]);
    assert_all_columns_present(&out, 2, "cat --where (DataFusion path)");
}

/// A rename the user is never told about is still a surprise. The note must
/// be on stderr (stdout stays machine-readable) and must name the column.
#[test]
fn rename_is_announced_on_stderr_not_stdout() {
    let dir = TempDir::new().unwrap();
    let f = dup_parquet(dir.path(), "dup2", &["id", "id"]);
    let (stdout, stderr) = run_ok(&[
        "sql",
        &format!("SELECT * FROM '{}'", f.to_str().unwrap()),
        "-f",
        "csv",
    ]);
    assert!(
        stderr.contains("id_1"),
        "the rename was not announced on stderr — it is silent again: {stderr:?}"
    );
    assert!(
        stderr.contains("duplicate"),
        "the stderr note does not say why the column was renamed: {stderr:?}"
    );
    assert!(
        !String::from_utf8_lossy(&stdout).contains("duplicate"),
        "the note leaked into stdout, corrupting machine-readable output"
    );
}

// ---------------------------------------------------------------------------
// Controls: the paths that already worked must keep working, and files
// without duplicates must be untouched by any of this.
// ---------------------------------------------------------------------------

#[test]
fn control_cat_and_export_still_keep_original_duplicate_names() {
    let dir = TempDir::new().unwrap();
    let f = dup_parquet(dir.path(), "dup2", &["id", "id"]);

    let (stdout, _) = run_ok(&["cat", f.to_str().unwrap(), "-f", "csv"]);
    let (header, rows) = parse_csv(&stdout);
    assert_eq!(
        header,
        vec!["id", "id"],
        "`cat -f csv` must keep the file's own names — it does not plan a query"
    );
    assert_eq!(
        rows,
        vec![
            vec![expected_col(0)[0].clone(), expected_col(1)[0].clone()],
            vec![expected_col(0)[1].clone(), expected_col(1)[1].clone()],
        ],
        "`cat -f csv` resolved the duplicate columns non-positionally"
    );

    let out = dir.path().join("out.csv");
    pq().args([
        "export",
        f.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ])
    .assert()
    .success();
    let (eheader, _) = parse_csv(&fs::read(&out).unwrap());
    assert_eq!(
        eheader,
        vec!["id", "id"],
        "`export` must keep the file's own names"
    );
}

/// A regression this fix's *mechanism* caused, and the guard against it
/// coming back. A local directory whose name ends in `.parquet` is a valid
/// DataFusion table — `ListingTable` reads every parquet file under it — and
/// the first version of the duplicate check tried to read that directory as a
/// single parquet file, turning a working query into
/// `Error: ... Is a directory (os error 21)`, exit 1.
///
/// This guard fails against that first implementation, which is what makes it
/// worth having: it does not test the original bug, it tests what the new
/// mechanism newly requires.
#[test]
fn a_directory_table_still_works() {
    let dir = TempDir::new().unwrap();
    let table_dir = dir.path().join("parts.parquet");
    fs::create_dir(&table_dir).unwrap();
    // Two files, so this exercises a real multi-file listing and schema
    // merge, not the degenerate one-file case.
    dup_parquet(&table_dir, "part0", &["a", "b", "c"]);
    dup_parquet(&table_dir, "part1", &["a", "b", "c"]);

    let (stdout, stderr) = run_ok(&[
        "sql",
        &format!("SELECT * FROM '{}'", table_dir.to_str().unwrap()),
        "-f",
        "csv",
    ]);
    let (header, rows) = parse_csv(&stdout);
    assert_eq!(
        header,
        vec!["a", "b", "c"],
        "a directory-of-parquet table stopped working"
    );
    assert_eq!(rows.len(), 4, "directory table returned no rows: {rows:?}");
    assert!(
        !stderr.contains("duplicate"),
        "a directory of unique-named files must not be warned about or \
         refused: {stderr:?}"
    );
}

#[test]
fn control_file_without_duplicates_is_untouched_and_silent() {
    let dir = TempDir::new().unwrap();
    let f = dup_parquet(dir.path(), "unique", &["a", "b", "c"]);
    let (stdout, stderr) = run_ok(&[
        "sql",
        &format!("SELECT * FROM '{}'", f.to_str().unwrap()),
        "-f",
        "csv",
    ]);
    let (header, _) = parse_csv(&stdout);
    assert_eq!(
        header,
        vec!["a", "b", "c"],
        "a file with unique names must be renamed by nothing"
    );
    assert!(
        !stderr.contains("duplicate"),
        "a duplicate-column note was printed for a file with no duplicates: {stderr:?}"
    );
    assert_all_columns_present(&stdout, 3, "control: unique names");
}

/// Aggregates and other planning that never touches the duplicate column
/// must still work on such a file.
#[test]
fn sql_aggregate_over_duplicate_column_file_works() {
    let dir = TempDir::new().unwrap();
    let f = dup_parquet(dir.path(), "dup2", &["id", "id"]);
    let (out, _) = run_ok(&[
        "sql",
        &format!("SELECT count(*) AS n FROM '{}'", f.to_str().unwrap()),
        "-f",
        "csv",
    ]);
    let (h, rows) = parse_csv(&out);
    assert_eq!(h, vec!["n"]);
    assert_eq!(rows, vec![vec!["2".to_string()]]);
}

// ---------------------------------------------------------------------------
// Directory tables (BUG 1): the same bytes must not answer correctly through
// one path and silently wrongly through another.
// ---------------------------------------------------------------------------
//
// Reproduced before the fix, one directory holding one file whose two int64
// columns are both named `id`, holding [1,2,3] and [10,20,30]:
//
//   pq sql "SELECT * FROM '.../dir.parquet/part0.parquet'"   (the file)
//     note: ... renamed column 2 'id' -> 'id_1'
//     {"id":1,"id_1":10}  {"id":2,"id_1":20}  {"id":3,"id_1":30}   rc=0
//
//   pq sql "SELECT * FROM '.../dir.parquet'"                 (the directory)
//     {"id":10}  {"id":20}  {"id":30}                              rc=0
//
// One column gone, the survivor carrying the *second* column's data under the
// *first* column's name, exit 0, no note. Ground truth from pyarrow 21.0.0:
// `ParquetFile(...).read()` on that file gives names ['id','id'] with
// [1,2,3] and [10,20,30], and `pyarrow.parquet.read_table` on the directory
// raises `ArrowInvalid: Can't unify schema with duplicate field names` — pq
// was answering where the reference implementation declines.
//
// pq now refuses too. These guards assert the refusal is loud (non-zero exit,
// a message naming the file and the reason) and that it never emits data.

/// Run `pq` expecting failure, returning (stdout, stderr). Both are returned
/// because a refusal that still printed rows would be worse than the bug.
fn run_fail(args: &[&str]) -> (Vec<u8>, String) {
    let out = pq().args(args).assert().failure().get_output().clone();
    (out.stdout, String::from_utf8_lossy(&out.stderr).to_string())
}

/// Assert a refusal is usable: it names the offending file, says why, and
/// emitted no rows.
fn assert_refused(stdout: &[u8], stderr: &str, offending_file: &str, case: &str) {
    assert!(
        stderr.contains("duplicate"),
        "[{case}] the refusal does not say the columns are duplicated: {stderr:?}"
    );
    assert!(
        stderr.contains(offending_file),
        "[{case}] the refusal does not name the offending file {offending_file:?}: {stderr:?}"
    );
    assert!(
        stdout.is_empty(),
        "[{case}] pq refused and still wrote rows to stdout: {:?}",
        String::from_utf8_lossy(stdout)
    );
}

#[test]
fn directory_with_a_duplicate_column_file_is_refused_not_silently_wrong() {
    let dir = TempDir::new().unwrap();
    let table_dir = dir.path().join("dir.parquet");
    fs::create_dir(&table_dir).unwrap();
    let file = dup_parquet(&table_dir, "part0", &["id", "id"]);

    // Instrument check first: the *file* route on the same bytes still
    // answers, and answers with both columns. If this half fails the fixture
    // is broken, not the directory handling.
    let (file_out, _) = run_ok(&[
        "sql",
        &format!("SELECT * FROM '{}'", file.to_str().unwrap()),
        "-f",
        "csv",
    ]);
    assert_all_columns_present(&file_out, 2, "the file inside the directory");

    let (stdout, stderr) = run_fail(&[
        "sql",
        &format!("SELECT * FROM '{}'", table_dir.to_str().unwrap()),
        "-f",
        "csv",
    ]);
    assert_refused(
        &stdout,
        &stderr,
        "part0.parquet",
        "directory of one dup file",
    );
    // The specific silent-wrong-answer shape must be gone, not merely
    // reworded: no row of the second column's data may appear anywhere.
    assert!(
        !String::from_utf8_lossy(&stdout).contains(&expected_col(1)[0]),
        "the directory route still emitted the second column's data: {:?}",
        String::from_utf8_lossy(&stdout)
    );
}

#[test]
fn directory_of_several_duplicate_column_files_is_refused() {
    let dir = TempDir::new().unwrap();
    let table_dir = dir.path().join("dir.parquet");
    fs::create_dir(&table_dir).unwrap();
    dup_parquet(&table_dir, "part0", &["id", "id"]);
    dup_parquet(&table_dir, "part1", &["id", "id"]);

    let (stdout, stderr) = run_fail(&[
        "sql",
        &format!("SELECT * FROM '{}'", table_dir.to_str().unwrap()),
        "-f",
        "csv",
    ]);
    assert!(
        stderr.contains("duplicate"),
        "several duplicate files were not refused: {stderr:?}"
    );
    assert!(stdout.is_empty(), "refused and still wrote rows");
}

/// The duplicate file need not be the first one listed, and the presence of
/// well-formed siblings must not launder it.
#[test]
fn directory_mixing_unique_and_duplicate_column_files_is_refused() {
    let dir = TempDir::new().unwrap();
    let table_dir = dir.path().join("dir.parquet");
    fs::create_dir(&table_dir).unwrap();
    dup_parquet(&table_dir, "aaa_ok", &["id", "other"]);
    dup_parquet(&table_dir, "zzz_bad", &["id", "id"]);

    let (stdout, stderr) = run_fail(&[
        "sql",
        &format!("SELECT * FROM '{}'", table_dir.to_str().unwrap()),
        "-f",
        "csv",
    ]);
    assert_refused(&stdout, &stderr, "zzz_bad.parquet", "mixed directory");
}

/// `pq cat --where` goes through the same registration (`query_with_where`),
/// so it had the identical silent wrong answer and must refuse identically.
#[test]
fn cat_where_on_a_duplicate_column_directory_is_refused() {
    let dir = TempDir::new().unwrap();
    let table_dir = dir.path().join("dir.parquet");
    fs::create_dir(&table_dir).unwrap();
    dup_parquet(&table_dir, "part0", &["id", "id"]);

    let (stdout, stderr) = run_fail(&[
        "cat",
        table_dir.to_str().unwrap(),
        "--where",
        "1 = 1",
        "-f",
        "csv",
    ]);
    assert_refused(
        &stdout,
        &stderr,
        "part0.parquet",
        "cat --where on a directory",
    );
}

/// Hive-partition subdirectories *are* read by DataFusion (segments
/// containing `=` survive `listing_table_ignore_subdirectory`), so a
/// duplicate hidden one level down under `k=1/` is still answered wrongly and
/// must still be refused. Measured on the release binary: a directory holding
/// `top.parquet`, `k=1/hive.parquet` and `nested/deep.parquet` returns rows
/// from the first two only.
#[test]
fn duplicate_columns_in_a_hive_partition_are_refused() {
    let dir = TempDir::new().unwrap();
    let table_dir = dir.path().join("dir.parquet");
    let part = table_dir.join("k=1");
    fs::create_dir_all(&part).unwrap();
    dup_parquet(&table_dir, "top", &["id", "other"]);
    dup_parquet(&part, "hive", &["id", "id"]);

    let (stdout, stderr) = run_fail(&[
        "sql",
        &format!("SELECT * FROM '{}'", table_dir.to_str().unwrap()),
        "-f",
        "csv",
    ]);
    assert_refused(&stdout, &stderr, "hive.parquet", "hive partition");
}

/// The other half of the same contract, and the guard against over-refusal:
/// a plain (non-Hive) subdirectory is *not* read by DataFusion, so a
/// duplicate-named file sitting there cannot corrupt the answer and must not
/// be refused. A hand-rolled recursive walk would fail this test; a flat one
/// would fail the Hive test above. Only asking DataFusion for its own file
/// list passes both.
#[test]
fn a_subdirectory_datafusion_never_reads_does_not_trigger_a_refusal() {
    let dir = TempDir::new().unwrap();
    let table_dir = dir.path().join("dir.parquet");
    let nested = table_dir.join("nested");
    fs::create_dir_all(&nested).unwrap();
    dup_parquet(&table_dir, "top", &["a", "b"]);
    dup_parquet(&nested, "deep", &["id", "id"]);

    let (stdout, stderr) = run_ok(&[
        "sql",
        &format!("SELECT * FROM '{}'", table_dir.to_str().unwrap()),
        "-f",
        "csv",
    ]);
    let (header, rows) = parse_csv(&stdout);
    assert_eq!(
        header,
        vec!["a", "b"],
        "an unread subdirectory changed the table's schema"
    );
    assert_eq!(
        rows.len(),
        2,
        "an unread subdirectory changed the rows returned: {rows:?}"
    );
    assert!(
        !stderr.contains("duplicate"),
        "refused on a file DataFusion never reads: {stderr:?}"
    );
}

/// Control: a directory whose files have *different* schemas that merge
/// cleanly must keep working. DataFusion unions them and fills the gaps with
/// nulls; the duplicate check must not disturb that.
#[test]
fn a_directory_of_different_but_mergeable_schemas_still_works() {
    let dir = TempDir::new().unwrap();
    let table_dir = dir.path().join("merge.parquet");
    fs::create_dir(&table_dir).unwrap();
    nullable_parquet(&table_dir, "part0", &["a", "b"]);
    nullable_parquet(&table_dir, "part1", &["a", "c"]);

    let (stdout, stderr) = run_ok(&[
        "sql",
        &format!("SELECT * FROM '{}' ORDER BY a", table_dir.to_str().unwrap()),
        "-f",
        "csv",
    ]);
    let (header, rows) = parse_csv(&stdout);
    let mut sorted = header.clone();
    sorted.sort();
    assert_eq!(
        sorted,
        vec!["a", "b", "c"],
        "differing-but-mergeable schemas stopped merging: {header:?}"
    );
    assert_eq!(rows.len(), 4, "mergeable directory lost rows: {rows:?}");
    assert!(
        !stderr.contains("duplicate"),
        "a mergeable-schema directory was warned about or refused: {stderr:?}"
    );
}
