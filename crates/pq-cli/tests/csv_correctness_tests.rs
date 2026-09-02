//! Guards for two CSV data-corruption classes, both silent (exit 0):
//!
//! BUG 1 — a CSV header frozen from the first row/batch, with no per-row key
//! lookup, when later rows carry a *different key set* than row 0 (e.g.
//! combining Parquet files with different schemas). This shifts a value
//! into the wrong-named column, or drops it entirely if row 0 never had
//! that column at all. It has nothing to do with key *order*: `serde_json`
//! here has no `preserve_order` feature, so `Value::Object` is a `BTreeMap`
//! and iterates alphabetically regardless of input order — a pure reorder
//! cannot shift anything. Every one of these tests fixes that by using a
//! *different key set* between inputs, not a different key order.
//!
//! These are exercised through every reachable emission path, because they
//! were three (in fact four) independent hand-rolled implementations of the
//! same bug, not one:
//!   - batch path: `pq cat a.parquet b.parquet --output out.csv`
//!   - export path: `pq export a.parquet b.parquet --output out.csv`
//!   - values/jq path: `pq cat a.parquet b.parquet --jq . -O out.csv`
//!   - stdout render path: `pq cat a.parquet b.parquet -f csv`
//!
//! BUG 2 — three hand-rolled CSV escapers each tested only `,`, `"`, and
//! `\n`, leaving a bare `\r` unquoted. A lone `\r` is just as much a record
//! separator to a compliant CSV reader as `\n`, so one logical row becomes
//! two. `\r\n` was already handled correctly (caught by the `\n` check);
//! only the *lone* `\r` case was broken. Exercised through the same four
//! paths above.
//!
//! Assertions parse the emitted bytes with the `csv` crate (the same crate
//! the fix now uses) in strict (non-flexible) mode, which is exactly the
//! kind of "compliant reader" the task described as tripping over this
//! output — never on exit codes, which are 0 in every case, fixed or not.

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn pq() -> Command {
    assert_cmd::cargo_bin_cmd!("pq")
}

/// Import one JSONL file (one JSON object per line) into a parquet file at
/// `dest`. Everything lives under the caller's `TempDir` — nothing touches
/// the shared `tests/fixtures` tree.
fn import_jsonl(dir: &Path, name: &str, jsonl: &str) -> PathBuf {
    let src = dir.join(format!("{name}.src.jsonl"));
    fs::write(&src, jsonl).unwrap();
    let out = dir.join(format!("{name}.parquet"));
    pq().args(["import", src.to_str().unwrap(), "-o", out.to_str().unwrap()])
        .assert()
        .success();
    out
}

/// Strict (RFC-4180-ish) CSV parse: every record must have the same field
/// count as the header, and quoting must be well-formed. This is the "any
/// compliant reader" the task describes — `csv::Reader` defaults to
/// non-flexible, so a ragged row is a parse error, not silently accepted.
fn parse_strict_csv(bytes: &[u8]) -> Result<(Vec<String>, Vec<Vec<String>>), csv::Error> {
    let mut rdr = csv::ReaderBuilder::new().from_reader(bytes);
    let header: Vec<String> = rdr.headers()?.iter().map(|s| s.to_string()).collect();
    let mut rows = Vec::new();
    for result in rdr.records() {
        let record = result?;
        rows.push(record.iter().map(|s| s.to_string()).collect());
    }
    Ok((header, rows))
}

// ---------------------------------------------------------------------------
// BUG 1: header-drift / key-set mismatch across the four emission paths
// ---------------------------------------------------------------------------

/// Two files with disjoint extra columns (id,name vs id,val). Row 0 (from
/// the first file) has keys {id,name}; a later row has keys {id,val}. A
/// header frozen from row 0 either has no `val` column at all (value
/// silently dropped) or writes `val`'s value into the `name` position
/// (value silently shifted) depending on the exact code path.
fn two_schema_files(dir: &Path) -> (PathBuf, PathBuf) {
    let a = import_jsonl(
        dir,
        "a",
        "{\"id\": 1, \"name\": \"alice\"}\n{\"id\": 2, \"name\": \"bob\"}\n",
    );
    let b = import_jsonl(dir, "b", "{\"id\": 3, \"val\": 30.5}\n");
    (a, b)
}

fn assert_union_header_no_drop_no_shift(csv_bytes: &[u8], case: &str) {
    let (header, rows) = parse_strict_csv(csv_bytes).unwrap_or_else(|e| {
        panic!(
            "[{case}] output is not valid strict CSV: {e}\nbytes: {:?}",
            String::from_utf8_lossy(csv_bytes)
        )
    });

    // The header must include every column from every input, not just row
    // 0's. Order isn't asserted (first-seen union is an implementation
    // choice, not a contract) but membership is.
    assert!(
        header.contains(&"id".to_string()),
        "[{case}] header missing 'id': {header:?}"
    );
    assert!(
        header.contains(&"name".to_string()),
        "[{case}] header missing 'name' (row-0-only header would still have this) : {header:?}"
    );
    assert!(
        header.contains(&"val".to_string()),
        "[{case}] header missing 'val' — the second file's column was dropped: {header:?}"
    );

    let id_idx = header.iter().position(|h| h == "id").unwrap();
    let name_idx = header.iter().position(|h| h == "name").unwrap();
    let val_idx = header.iter().position(|h| h == "val").unwrap();

    // Find the row for id=3 (from file b, which has no `name`).
    let row3 = rows
        .iter()
        .find(|r| r[id_idx] == "3")
        .unwrap_or_else(|| panic!("[{case}] no row with id=3 in {rows:?}"));

    // The defining assertion: 30.5 must show up under the `val` column,
    // never under `name` (that's the shift) and never nowhere at all
    // (that's the drop).
    assert_eq!(
        row3[val_idx], "30.5",
        "[{case}] value 30.5 did not land in the 'val' column: row={row3:?} header={header:?}"
    );
    assert_ne!(
        row3[name_idx], "30.5",
        "[{case}] value 30.5 leaked into the 'name' column — this is the exact column-shift bug: row={row3:?} header={header:?}"
    );
}

#[test]
fn csv_batch_path_header_is_union_not_row_zero() {
    let dir = TempDir::new().unwrap();
    let (a, b) = two_schema_files(dir.path());
    let out = dir.path().join("out.csv");
    pq().args([
        "cat",
        a.to_str().unwrap(),
        b.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ])
    .assert()
    .success();
    let bytes = fs::read(&out).unwrap();
    assert_union_header_no_drop_no_shift(&bytes, "cat --output (batch path)");
}

#[test]
fn csv_export_path_header_is_union_not_row_zero() {
    let dir = TempDir::new().unwrap();
    let (a, b) = two_schema_files(dir.path());
    let out = dir.path().join("out.csv");
    pq().args([
        "export",
        a.to_str().unwrap(),
        b.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ])
    .assert()
    .success();
    let bytes = fs::read(&out).unwrap();
    assert_union_header_no_drop_no_shift(&bytes, "export --output");
}

#[test]
fn csv_values_jq_path_header_is_union_not_row_zero() {
    let dir = TempDir::new().unwrap();
    let (a, b) = two_schema_files(dir.path());
    let out = dir.path().join("out.csv");
    pq().args([
        "cat",
        a.to_str().unwrap(),
        b.to_str().unwrap(),
        "--jq",
        ".",
        "-O",
        out.to_str().unwrap(),
    ])
    .assert()
    .success();
    let bytes = fs::read(&out).unwrap();
    assert_union_header_no_drop_no_shift(&bytes, "cat --jq . -O (values path)");
}

#[test]
fn csv_stdout_render_path_header_is_union_not_row_zero() {
    let dir = TempDir::new().unwrap();
    let (a, b) = two_schema_files(dir.path());
    let output = pq()
        .args(["cat", a.to_str().unwrap(), b.to_str().unwrap(), "-f", "csv"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_union_header_no_drop_no_shift(&output, "cat -f csv (stdout render path)");
}

/// Reverse order: the first row now has *fewer* keys ({id} only) than a
/// later row ({id,name,val}). A row-0-frozen header of just `id` makes the
/// later, wider row ragged — more fields than the header — which a strict
/// (non-flexible) reader rejects outright. This must parse cleanly and
/// keep every value.
#[test]
fn csv_narrow_then_wide_row_is_not_ragged() {
    let dir = TempDir::new().unwrap();
    let narrow = import_jsonl(dir.path(), "narrow", "{\"id\": 9}\n");
    let wide = import_jsonl(
        dir.path(),
        "wide",
        "{\"id\": 10, \"name\": \"carol\", \"val\": 1.5}\n",
    );
    let out = dir.path().join("out.csv");
    pq().args([
        "cat",
        narrow.to_str().unwrap(),
        wide.to_str().unwrap(),
        "--jq",
        ".",
        "-O",
        out.to_str().unwrap(),
    ])
    .assert()
    .success();
    let bytes = fs::read(&out).unwrap();
    let (header, rows) = parse_strict_csv(&bytes).unwrap_or_else(|e| {
        panic!(
            "output is not valid strict CSV (ragged rows): {e}\nbytes: {:?}",
            String::from_utf8_lossy(&bytes)
        )
    });
    assert_eq!(rows.len(), 2, "expected 2 data rows, got {rows:?}");
    let id_idx = header.iter().position(|h| h == "id").unwrap();
    let val_idx = header.iter().position(|h| h == "val").unwrap();
    let row10 = rows.iter().find(|r| r[id_idx] == "10").unwrap();
    assert_eq!(row10[val_idx], "1.5", "val dropped/misplaced: {row10:?}");
}

// ---------------------------------------------------------------------------
// BUG 2: a bare CR must be quoted, across the same four paths
// ---------------------------------------------------------------------------

fn cr_fixture(dir: &Path) -> PathBuf {
    // A literal carriage return inside the string (JSON \r escape), no
    // following \n. \r\n is a separate, already-correctly-handled case
    // (caught incidentally by the `\n` check) and is not what's under test
    // here.
    import_jsonl(
        dir,
        "cr",
        "{\"id\": 1, \"note\": \"has\\rCR\"}\n{\"id\": 2, \"note\": \"plain\"}\n",
    )
}

fn assert_cr_quoted_and_two_rows(csv_bytes: &[u8], case: &str) {
    // Byte-level check first: a compliant strict-CSV parse of exactly 2
    // data rows is only possible if the lone \r was quoted. If it wasn't,
    // the \r itself terminates a record mid-field and this either fails to
    // parse as strict CSV or (if it happens to still "parse") yields 3
    // rows instead of 2 with the note field split in half.
    let (header, rows) = parse_strict_csv(csv_bytes).unwrap_or_else(|e| {
        panic!(
            "[{case}] failed to parse as strict CSV — a bare \\r likely broke record framing: {e}\nraw bytes: {:?}",
            String::from_utf8_lossy(csv_bytes)
        )
    });
    assert_eq!(
        rows.len(),
        2,
        "[{case}] expected exactly 2 data rows (a bare \\r split one row into two): header={header:?} rows={rows:?}"
    );
    let note_idx = header.iter().position(|h| h == "note").unwrap();
    let id_idx = header.iter().position(|h| h == "id").unwrap();
    let row1 = rows.iter().find(|r| r[id_idx] == "1").unwrap();
    assert_eq!(
        row1[note_idx], "has\rCR",
        "[{case}] the CR did not survive as part of one field's value: {row1:?}"
    );

    // Byte-level confirmation that the field was actually quoted (not just
    // "happened to parse"): the raw output must contain the CR wrapped in
    // double quotes, `"has\rCR"`, not bare `has\rCR`.
    let quoted = b"\"has\rCR\"";
    let bare = b"1,has\rCR\n";
    assert!(
        csv_bytes
            .windows(quoted.len())
            .any(|w| w == quoted.as_slice()),
        "[{case}] expected the quoted byte sequence {:?} in output, got: {:?}",
        String::from_utf8_lossy(quoted),
        String::from_utf8_lossy(csv_bytes)
    );
    assert!(
        !csv_bytes.windows(bare.len()).any(|w| w == bare.as_slice()),
        "[{case}] found the UNQUOTED bare-CR byte sequence {:?} — this is exactly the bug",
        String::from_utf8_lossy(bare)
    );
}

#[test]
fn csv_batch_path_quotes_bare_cr() {
    let dir = TempDir::new().unwrap();
    let f = cr_fixture(dir.path());
    let out = dir.path().join("out.csv");
    pq().args([
        "cat",
        f.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ])
    .assert()
    .success();
    let bytes = fs::read(&out).unwrap();
    assert_cr_quoted_and_two_rows(&bytes, "cat --output (batch path)");
}

#[test]
fn csv_export_path_quotes_bare_cr() {
    let dir = TempDir::new().unwrap();
    let f = cr_fixture(dir.path());
    let out = dir.path().join("out.csv");
    pq().args([
        "export",
        f.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ])
    .assert()
    .success();
    let bytes = fs::read(&out).unwrap();
    assert_cr_quoted_and_two_rows(&bytes, "export --output");
}

#[test]
fn csv_values_jq_path_quotes_bare_cr() {
    let dir = TempDir::new().unwrap();
    let f = cr_fixture(dir.path());
    let out = dir.path().join("out.csv");
    pq().args([
        "cat",
        f.to_str().unwrap(),
        "--jq",
        ".",
        "-O",
        out.to_str().unwrap(),
    ])
    .assert()
    .success();
    let bytes = fs::read(&out).unwrap();
    assert_cr_quoted_and_two_rows(&bytes, "cat --jq . -O (values path)");
}

#[test]
fn csv_stdout_render_path_quotes_bare_cr() {
    let dir = TempDir::new().unwrap();
    let f = cr_fixture(dir.path());
    let output = pq()
        .args(["cat", f.to_str().unwrap(), "-f", "csv"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_cr_quoted_and_two_rows(&output, "cat -f csv (stdout render path)");
}

/// Control: `\r\n` was already quoted correctly before this fix (the `\n`
/// check happened to catch it). This guards against a regression where the
/// new implementation stops quoting CRLF while fixing the lone-CR case.
#[test]
fn csv_crlf_still_quoted_control() {
    let dir = TempDir::new().unwrap();
    let f = import_jsonl(
        dir.path(),
        "crlf",
        "{\"id\": 1, \"note\": \"has\\r\\nCRLF\"}\n",
    );
    let out = dir.path().join("out.csv");
    pq().args([
        "cat",
        f.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ])
    .assert()
    .success();
    let bytes = fs::read(&out).unwrap();
    let (header, rows) = parse_strict_csv(&bytes).unwrap();
    assert_eq!(rows.len(), 1);
    let note_idx = header.iter().position(|h| h == "note").unwrap();
    assert_eq!(rows[0][note_idx], "has\r\nCRLF");
}
