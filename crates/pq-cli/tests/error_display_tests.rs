//! Guards for the "pq prints the same error sentence twice" class of bug
//! (TODO.md P2). `PqError`'s `Display` used to interpolate its own
//! `source`'s text into its message *and* implement `source()` (via
//! `#[source]`/`#[from]`), so `pq-cli/src/main.rs`'s `eprintln!("Error:
//! {e:#}")` — which uses `anyhow`'s alternate `Display` to walk the full
//! source chain — printed the same text twice, e.g.:
//!
//!   Error: Failed to read parquet file 'x.parquet': EOF: file size of 0 is
//!   less than footer: EOF: file size of 0 is less than footer
//!
//! `crates/pq-core/src/error.rs` has unit tests that check this one hop at a
//! time (a variant's `Display` vs. its immediate `source()`). This file
//! checks the *end-to-end* shape instead: run real failing commands through
//! the actual built binary and assert the printed message never contains a
//! sentence immediately followed by a `": "`-joined repeat of itself. That
//! is call-site- and wording-agnostic — it would catch this bug shape in any
//! command, present or future, not just the ones exercised below.

use assert_cmd::Command;
use std::fs;
use tempfile::TempDir;

fn pq() -> Command {
    assert_cmd::cargo_bin_cmd!("pq")
}

/// Finds a `": "`-joined immediate self-repeat in `s`: a substring `T` (at
/// least `MIN_LEN` bytes, to avoid false positives on short incidental
/// repeats like a shared word) that is directly followed by `": "` and then
/// `T` again. This is exactly the shape `anyhow`'s `{:#}` produces when a
/// `PqError` variant's own `Display` already embeds its source's full text:
/// the source's text ends up printed once by the variant, then a second time
/// by the chain walk, joined by `anyhow`'s `": "` separator.
fn find_doubled_sentence(s: &str) -> Option<&str> {
    const MIN_LEN: usize = 8;
    let n = s.len();
    for i in 0..n {
        // Only try splits on char boundaries; skip otherwise.
        if !s.is_char_boundary(i) {
            continue;
        }
        let max_j = n.saturating_sub(2); // need room for ": " + repeat after j
        let mut j = i + MIN_LEN;
        while j <= max_j {
            if !s.is_char_boundary(j) {
                j += 1;
                continue;
            }
            let candidate = &s[i..j];
            if s[j..].starts_with(": ") {
                let rest = &s[j + 2..];
                if rest.starts_with(candidate) {
                    return Some(candidate);
                }
            }
            j += 1;
        }
    }
    None
}

/// A run of this checker against a string that never reached a real failure
/// (e.g. empty stderr from a subject that silently exited 0) must not read
/// as "no doubling found" == pass. Callers therefore always assert stderr is
/// non-empty and starts with "Error:" before calling this, so a broken
/// harness fails loudly instead of vacuously passing.
fn assert_error_shape_ok(stderr: &str, must_contain: &[&str]) {
    assert!(
        stderr.starts_with("Error:"),
        "expected a top-level `Error: ...` line, got: {stderr:?}"
    );
    if let Some(doubled) = find_doubled_sentence(stderr) {
        panic!(
            "error message repeats the same sentence twice (joined by \": \"): {doubled:?}\n\
             full message: {stderr:?}"
        );
    }
    for needle in must_contain {
        assert!(
            stderr.contains(needle),
            "expected context {needle:?} to survive in the error message: {stderr:?}"
        );
    }
}

fn stderr_of(cmd: &mut Command) -> String {
    let output = cmd.output().expect("failed to run pq");
    assert!(
        !output.status.success(),
        "expected this command to fail, but it exited 0"
    );
    String::from_utf8(output.stderr).expect("stderr was not valid UTF-8")
}

fn tmp() -> TempDir {
    TempDir::new().expect("failed to create temp dir")
}

#[test]
fn missing_file_error_is_not_doubled() {
    let dir = tmp();
    let missing = dir.path().join("nope.parquet");
    for sub in ["info", "schema", "head", "count"] {
        let stderr = stderr_of(pq().arg(sub).arg(&missing));
        assert_error_shape_ok(&stderr, &["nope.parquet"]);
    }
}

#[test]
fn zero_byte_parquet_error_is_not_doubled() {
    let dir = tmp();
    let path = dir.path().join("zero.parquet");
    fs::write(&path, b"").unwrap();
    let stderr = stderr_of(pq().arg("info").arg(&path));
    assert_error_shape_ok(&stderr, &["zero.parquet"]);
}

#[test]
fn truncated_parquet_error_is_not_doubled() {
    let dir = tmp();
    let path = dir.path().join("garbage.parquet");
    // Not a valid parquet footer at any size: triggers a corrupt-footer
    // ParquetError rather than the too-small-for-footer EOF error, giving
    // coverage of a second distinct ParquetRead source shape.
    fs::write(&path, vec![0u8; 64]).unwrap();
    let stderr = stderr_of(pq().arg("info").arg(&path));
    assert_error_shape_ok(&stderr, &["garbage.parquet"]);
}

#[test]
fn malformed_json_import_error_is_not_doubled() {
    let dir = tmp();
    let src = dir.path().join("bad.jsonl");
    fs::write(&src, b"{not valid json\n").unwrap();
    let out = dir.path().join("out.parquet");
    let stderr = stderr_of(pq().arg("import").arg(&src).arg("-o").arg(&out));
    assert_error_shape_ok(&stderr, &[]);
}

#[test]
fn import_to_a_directory_destination_error_is_not_doubled() {
    // Deterministic stand-in for TODO.md P2's named repro
    // (`import x.jsonl -o /dev/stdout > out.bin`): both hit the exact same
    // bug, `PqError::Io`'s `#[from]` doubling a plain `std::io::Error`'s
    // text. `-o` pointing at an existing directory reaches it via a plain,
    // OS-level `EISDIR` with no filesystem-race surface, unlike
    // `/dev/stdout` (see `dev_stdout_repro_is_flaky_under_concurrent_load_see_comment`
    // below for why that one isn't used as the load-bearing guard).
    let dir = tmp();
    let src = dir.path().join("x.jsonl");
    fs::write(&src, b"{\"a\":1}\n").unwrap();
    let existing_dir = dir.path().join("adir");
    fs::create_dir(&existing_dir).unwrap();
    let stderr = stderr_of(pq().arg("import").arg(&src).arg("-o").arg(&existing_dir));
    assert_error_shape_ok(&stderr, &[]);
}

#[test]
fn dev_stdout_repro_now_succeeds_instead_of_doubling_an_error() {
    // This used to be the literal repro from TODO.md's P2 entry:
    // `pq import x.jsonl -o /dev/stdout > out.bin`, kept `#[ignore]`d because
    // whether it doubled an error message depended on an unrelated,
    // independently-flaky bug: `output_guard.rs::can_stage` resolved
    // `/dev/stdout` to `/dev/fd/1`, saw `fs::metadata` report a regular file
    // (because fd 1 was redirected to one), and tried to stage a sibling
    // temp file inside the synthetic `/dev/fd` directory, which fails with
    // ENOENT — under concurrent test load that flipped the exit code ~1 run
    // in 5, so it never became the load-bearing guard for the doubling bug
    // (`import_to_a_directory_destination_error_is_not_doubled` above is).
    //
    // That bug is now fixed in `output_guard.rs::can_stage`
    // (`is_descriptor_alias`), so this command no longer errors at all —
    // there is nothing left to double. This test is the corrected claim:
    // `import -o /dev/stdout` succeeds, deterministically, and the data
    // makes it through, redirected to a real regular file exactly as the
    // original repro did.
    let dir = tmp();
    let src = dir.path().join("x.jsonl");
    fs::write(&src, b"{\"a\":1}\n").unwrap();
    let out_file = dir.path().join("captured_stdout.bin");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_pq"))
        .arg("import")
        .arg(&src)
        .arg("-o")
        .arg("/dev/stdout")
        .stdout(std::process::Stdio::from(
            fs::File::create(&out_file).unwrap(),
        ))
        .output()
        .expect("failed to run pq");
    let stderr = String::from_utf8(output.stderr).expect("stderr was not valid UTF-8");
    assert!(
        output.status.success(),
        "import -o /dev/stdout should now succeed; stderr: {stderr}"
    );
    let bytes = fs::read(&out_file).unwrap();
    assert!(
        bytes.len() >= 4 && &bytes[..4] == b"PAR1",
        "import -o /dev/stdout: captured output is not Parquet: {:?}",
        &bytes[..bytes.len().min(20)]
    );
}

/// Writes a tiny parquet fixture (columns `id: Int64`, `name: Utf8`, two
/// rows) into `dir`, for the `pq sql` error-shape tests below. Built with
/// arrow/parquet directly, same approach as `sql_duplicate_columns_tests.rs`.
fn small_parquet(dir: &std::path::Path) -> std::path::PathBuf {
    use arrow::array::{Int64Array, RecordBatch, StringArray};
    use arrow::datatypes::{DataType, Field, Schema};
    use std::sync::Arc;

    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("name", DataType::Utf8, false),
    ]));
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(Int64Array::from(vec![1, 2])),
            Arc::new(StringArray::from(vec!["alice", "bob"])),
        ],
    )
    .unwrap();
    let out = dir.join("small.parquet");
    let file = fs::File::create(&out).unwrap();
    let mut writer = parquet::arrow::ArrowWriter::try_new(file, schema, None).unwrap();
    writer.write(&batch).unwrap();
    writer.close().unwrap();
    out
}

/// `pq sql`'s error chain used to triple, not double: `SqlError::DataFusion`
/// (`crates/pq-query/src/sql.rs`) both interpolated `datafusion::DataFusionError`'s
/// full `Display` into its own message *and* derived `source()` from it via
/// `#[from]`, so `anyhow`'s `{:#}` chain walk printed that same text a second
/// time — and then, for `DataFusionError` variants that themselves wrap a
/// further error (`SQL`, `ArrowError`, `SchemaError`), a third time via one
/// more `source()` hop, e.g.:
///
///   Error: DataFusion error: SQL error: ParserError("..."): SQL error:
///   ParserError("..."): sql parser error: ...
///
/// A different mechanism from the `pq-core` doubling (DIARY.md 2026-09-02):
/// there, the fix stopped a `Display` from embedding its own source's text.
/// Here `DataFusionError`'s `Display` is already the complete, self-contained
/// rendering of the whole nested error by the external crate's own design —
/// the fix instead severs `SqlError::DataFusion`'s `source()` link entirely
/// (a hand-written `From` impl, no `#[from]`/`#[source]`), so `anyhow` has
/// nothing left to chain-walk into. This test covers the parser, planning,
/// schema, and type-error families, since each wraps a different
/// `DataFusionError` variant and the doubling mechanism inside DataFusion's
/// own `Display`/`source()` differs by variant (`Plan`/`Execution` have no
/// `source()` at all and only doubled via *our* bug; `SQL`/`ArrowError`/
/// `SchemaError` wrap a further error and would have tripled).
#[test]
fn sql_error_chains_are_not_doubled_or_tripled() {
    let dir = tmp();
    let file = small_parquet(dir.path());
    let file_str = file.to_str().unwrap();

    // 1. Parser error (SqlError::DataFusion(DataFusionError::SQL), which
    //    wraps a further sqlparser::ParserError with its own source()).
    let stderr = stderr_of(pq().arg("sql").arg("SELECT"));
    assert_error_shape_ok(&stderr, &["Expected"]);

    // 2. Unknown table (DataFusionError::Plan, a leaf with no source()).
    let stderr = stderr_of(pq().arg("sql").arg("SELECT * FROM nonexistent_table_xyz"));
    assert_error_shape_ok(&stderr, &["nonexistent_table_xyz"]);

    // 3. Unknown column (DataFusionError::SchemaError, which wraps a further
    //    SchemaError with its own source()). Context: the bad column name and
    //    at least one real column name must both survive.
    let query = format!("SELECT nope_col FROM '{file_str}'");
    let stderr = stderr_of(pq().arg("sql").arg(&query));
    assert_error_shape_ok(&stderr, &["nope_col", "id"]);

    // 4. Type error: an invalid CAST (DataFusionError::ArrowError, which
    //    wraps a further arrow::error::ArrowError with its own source()).
    //    Context: the offending value must survive.
    let query = format!("SELECT CAST(name AS INT) FROM '{file_str}'");
    let stderr = stderr_of(pq().arg("sql").arg(&query));
    assert_error_shape_ok(&stderr, &["alice"]);

    // 5. Type error in a predicate, via an arithmetic type mismatch
    //    (DataFusionError::Plan again, exercised through a WHERE-adjacent
    //    expression rather than a bare SELECT).
    let query = format!("SELECT * FROM '{file_str}' WHERE name + 1 > 0");
    let stderr = stderr_of(pq().arg("sql").arg(&query));
    assert_error_shape_ok(&stderr, &[]);

    // 6. Unknown function (DataFusionError::Plan).
    let query = format!("SELECT nonexistent_func_xyz(id) FROM '{file_str}'");
    let stderr = stderr_of(pq().arg("sql").arg(&query));
    assert_error_shape_ok(&stderr, &["nonexistent_func_xyz"]);
}

/// Regression guard: `arrow_schema_of` and `RenamedDuplicatesTable::materialize`
/// (`crates/pq-query/src/sql.rs`) converted `pq_core::error::PqError` into
/// `SqlError::Other` via a bare `e.to_string()`, which reads only `Display`
/// and never walks `source()`. Once the `pq-core` doubling fix (DIARY.md,
/// 2026-09-02) moved a `PqError` variant's cause out of `Display` and into
/// `source()` only, that flattening lost the cause entirely: `pq sql` and
/// `pq cat --where` reported "Failed to read parquet file 'x'" with no
/// indication of *why*, so a corrupt file, an empty file, and (elsewhere) a
/// permission-denied file all produced the identical, useless message.
/// `pq cat` itself was unaffected because it keeps the error as a real
/// `anyhow` chain instead of stringifying it early. Confirmed on the
/// pre-fix binary: both cases below printed only
/// `Failed to read parquet file '...'` with nothing after it.
#[test]
fn sql_and_cat_where_preserve_the_read_failure_cause() {
    let dir = tmp();
    let corrupt = dir.path().join("corrupt.parquet");
    fs::write(&corrupt, vec![0u8; 64]).unwrap();
    let empty = dir.path().join("empty.parquet");
    fs::write(&empty, b"").unwrap();

    // `sql`, corrupt footer (ParquetError, no further source()).
    let query = format!("SELECT * FROM '{}'", corrupt.to_str().unwrap());
    let stderr = stderr_of(pq().arg("sql").arg(&query));
    assert_error_shape_ok(&stderr, &["Corrupt footer"]);

    // `sql`, zero-byte file (a distinct PqError/ParquetError shape: too
    // small to even hold a footer).
    let query = format!("SELECT * FROM '{}'", empty.to_str().unwrap());
    let stderr = stderr_of(pq().arg("sql").arg(&query));
    assert_error_shape_ok(&stderr, &["too small"]);

    // `cat --where` reaches the same `arrow_schema_of` call as `sql` before
    // it ever reads a row, so the same flattening bug applied there too.
    let stderr = stderr_of(pq().arg("cat").arg(&corrupt).arg("--where").arg("1=1"));
    assert_error_shape_ok(&stderr, &["Corrupt footer"]);
}

#[test]
fn detector_catches_a_known_doubled_string() {
    // Guards the guard: if `find_doubled_sentence` regresses (e.g. someone
    // "simplifies" it into a no-op), this fails loudly rather than letting
    // the tests above pass vacuously.
    let doubled = "Error: Failed to read parquet file 'x.parquet': EOF: file size of 0 is less than footer: EOF: file size of 0 is less than footer";
    assert_eq!(
        find_doubled_sentence(doubled),
        Some("EOF: file size of 0 is less than footer")
    );
    let fine =
        "Error: Failed to read parquet file 'x.parquet': EOF: file size of 0 is less than footer";
    assert_eq!(find_doubled_sentence(fine), None);
}
