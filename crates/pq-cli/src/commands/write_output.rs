use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::Path;

use arrow::array::RecordBatch;
use arrow::datatypes::{Schema, SchemaRef};
use arrow::util::display::{ArrayFormatter, FormatOptions};

/// Output file format, auto-detected from extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFileFormat {
    Parquet,
    Json,
    JsonLines,
    Csv,
}

/// Run `body` against a buffered wrapper of `inner`, then flush and
/// propagate any flush error before returning `body`'s result.
///
/// This is the one place every buffered file-write in this module (and, via
/// re-export, `export.rs`) goes through, specifically so the
/// "write, then flush, then let the caller's `?` gate whatever runs next" ordering
/// lives in exactly one spot instead of being repeated — and potentially
/// gotten wrong — at every call site. Swallowing the flush's `Result`
/// (`let _ = buffered.flush();`) instead of propagating it is exactly the
/// mistake that matters here: every writer in this workspace goes through
/// `pq_transform::output_guard::with_atomic_output`, which renames the
/// staged file over the destination only *after* the writing closure
/// returns `Ok`. A swallowed flush error would let that `Ok` through with an
/// incompletely-written staged file, and the rename would commit it — a
/// silent truncation. See `write_buffered_propagates_a_flush_error` below,
/// which fails immediately if the `?` after `.flush()` is ever weakened to
/// an ignored result.
pub(crate) fn write_buffered<W, T>(
    inner: W,
    body: impl FnOnce(&mut std::io::BufWriter<W>) -> anyhow::Result<T>,
) -> anyhow::Result<T>
where
    W: Write,
{
    let mut buffered = std::io::BufWriter::new(inner);
    let value = body(&mut buffered)?;
    buffered.flush()?;
    Ok(value)
}

pub fn format_from_extension(path: &str) -> OutputFileFormat {
    match Path::new(path).extension().and_then(|e| e.to_str()) {
        Some("parquet") => OutputFileFormat::Parquet,
        Some("json") => OutputFileFormat::Json,
        Some("jsonl" | "ndjson") => OutputFileFormat::JsonLines,
        Some("csv") => OutputFileFormat::Csv,
        _ => OutputFileFormat::JsonLines,
    }
}

/// Write RecordBatches to a file in an **already-resolved** format.
/// Returns the number of rows written.
///
/// This exists so that a caller which has decided the format from the
/// destination the *user named* can hand that decision down instead of
/// letting a second function re-derive it from a different string.
/// `sql -o` used to resolve the format from the destination name and then
/// call [`write_batches_to_file`] with the **staging** path; since
/// `pq_transform::output_guard` builds the staging name from the resolved
/// *symlink target*, `-o link.parquet` where `link.parquet -> target.csv`
/// staged as `...csv`, the second sniff won, and CSV bytes landed under a
/// `.parquet` name with exit 0. Format is decided once; here it is only
/// obeyed.
pub fn write_batches_as(
    path: &Path,
    batches: &[RecordBatch],
    format: OutputFileFormat,
) -> anyhow::Result<usize> {
    let total_rows: usize = batches.iter().map(|b| b.num_rows()).sum();

    match format {
        OutputFileFormat::Parquet => {
            if batches.is_empty() {
                anyhow::bail!("No data to write");
            }
            let opts = pq_core::writer::WriteOptions::default();
            pq_core::writer::write_batches(path, batches, &opts)?;
        }
        text => {
            // Buffered: `write_batches_text`'s JSON Lines and CSV branches
            // write per row, which against a raw `File` is a syscall per
            // row. See `write_buffered` for why the flush must happen here,
            // before this function returns, rather than being left to the
            // `BufWriter`'s `Drop` impl (which discards its error).
            write_buffered(std::fs::File::create(path)?, |file| {
                write_batches_text(file, batches, text)?;
                Ok(())
            })?;
        }
    }

    Ok(total_rows)
}

/// Write RecordBatches to a file, **sniffing** the format from `path`'s
/// extension. Returns the number of rows written.
///
/// Only correct when `path` is the destination the user actually named.
/// Never call this with a staging path — see [`write_batches_as`] for why
/// that combination silently wrote the wrong format.
pub fn write_batches_to_file(path: &str, batches: &[RecordBatch]) -> anyhow::Result<usize> {
    write_batches_as(Path::new(path), batches, format_from_extension(path))
}

/// Write JSON values to a file, auto-detecting format from extension.
/// For Parquet output, infers schema and converts to RecordBatches first.
/// Returns the number of rows written.
pub fn json_values_to_file(path: &str, values: &[serde_json::Value]) -> anyhow::Result<usize> {
    match format_from_extension(path) {
        OutputFileFormat::Parquet => {
            // Filter to only object values for schema inference
            let objects: Vec<serde_json::Value> =
                values.iter().filter(|v| v.is_object()).cloned().collect();
            if objects.is_empty() {
                anyhow::bail!(
                    "No object data to write to Parquet (jq output must be JSON objects)"
                );
            }
            let schema = pq_transform::schema_inference::infer_schema_from_json(&objects)?;
            let batches =
                pq_transform::schema_inference::json_values_to_batches(&objects, &schema)?;
            let opts = pq_core::writer::WriteOptions::default();
            pq_core::writer::write_batches(Path::new(path), &batches, &opts)?;
            Ok(objects.len())
        }
        format => {
            // Buffered for the same reason as `write_batches_as`'s text
            // branch above; see `write_buffered`.
            write_buffered(std::fs::File::create(path)?, |file| {
                write_values_text(file, values, format)
            })?;
            Ok(values.len())
        }
    }
}

fn write_batches_text(
    writer: &mut dyn Write,
    batches: &[RecordBatch],
    format: OutputFileFormat,
) -> anyhow::Result<()> {
    match format {
        OutputFileFormat::Json => {
            let mut all_rows: Vec<serde_json::Value> = Vec::new();
            for batch in batches {
                all_rows.extend(pq_query::convert::batch_to_json_rows(batch));
            }
            serde_json::to_writer_pretty(&mut *writer, &all_rows)?;
            writeln!(writer)?;
        }
        OutputFileFormat::JsonLines => {
            for batch in batches {
                for row in pq_query::convert::batch_to_json_rows(batch) {
                    serde_json::to_writer(&mut *writer, &row)?;
                    writeln!(writer)?;
                }
            }
        }
        OutputFileFormat::Csv => {
            write_batches_csv(writer, batches)?;
        }
        OutputFileFormat::Parquet => unreachable!(),
    }
    Ok(())
}

fn write_values_text(
    writer: &mut dyn Write,
    values: &[serde_json::Value],
    format: OutputFileFormat,
) -> anyhow::Result<()> {
    match format {
        OutputFileFormat::Json => {
            serde_json::to_writer_pretty(&mut *writer, values)?;
            writeln!(writer)?;
        }
        OutputFileFormat::JsonLines => {
            for value in values {
                serde_json::to_writer(&mut *writer, value)?;
                writeln!(writer)?;
            }
        }
        OutputFileFormat::Csv => write_values_csv(writer, values)?,
        OutputFileFormat::Parquet => unreachable!(),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Batch-derived CSV. One implementation, shared by every path that turns
// Arrow batches into CSV: `cat -f csv` (stdout), `cat --output x.csv`,
// `export`, and `sql -o x.csv`. There used to be four; they disagreed.
// ---------------------------------------------------------------------------

/// One column of a batch-derived CSV: a field name, plus which field of that
/// name it is within a single schema.
///
/// **Why the occurrence index.** A Parquet file may legally carry two fields
/// with the same name, and an Arrow batch is *positional* — column 0 and
/// column 1 can both be called `id`. The first union-header implementation
/// deduped names through a `HashSet` and then resolved each header entry
/// back to data with `Schema::index_of(name)` (stdout) or a JSON map keyed
/// by name (file paths). Both keep only the **first** field of a given name,
/// so the second `id` column silently vanished — and the two paths did not
/// even drop the same one: `cat -f csv` emitted the first column's values,
/// `export -o` the second's.
///
/// Keying on `(name, occurrence)` restores positional identity within a
/// schema while preserving the reason the union header exists: aligning
/// *heterogeneous files* by column name. A column is "the same column"
/// across batches when both its name and its occurrence index match.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct CsvColumn {
    name: String,
    occurrence: usize,
}

/// The CSV columns for a set of schemas: the union over every schema, in
/// first-seen order.
///
/// `pq cat a.parquet b.parquet -f csv` can combine files with different
/// schemas; freezing the header from the first schema either misaligns a
/// later batch's values under the wrong column or silently drops a column
/// the first schema didn't have.
pub(crate) fn union_columns(schemas: impl IntoIterator<Item = SchemaRef>) -> Vec<CsvColumn> {
    let mut columns = Vec::new();
    let mut seen = HashSet::new();
    for schema in schemas {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for field in schema.fields() {
            let occurrence = counts.entry(field.name().clone()).or_insert(0);
            let column = CsvColumn {
                name: field.name().clone(),
                occurrence: *occurrence,
            };
            *occurrence += 1;
            if seen.insert(column.clone()) {
                columns.push(column);
            }
        }
    }
    columns
}

/// Where each CSV column lives in `schema`, or `None` when this schema
/// doesn't have it (a file that lacks a column another file has).
///
/// Resolution is positional: the *n*-th field named `id` answers for the
/// *n*-th `id` column, which is what `Schema::index_of` could not express.
fn column_indices(columns: &[CsvColumn], schema: &Schema) -> Vec<Option<usize>> {
    let mut positions: HashMap<&str, Vec<usize>> = HashMap::new();
    for (idx, field) in schema.fields().iter().enumerate() {
        positions
            .entry(field.name().as_str())
            .or_default()
            .push(idx);
    }
    columns
        .iter()
        .map(|column| {
            positions
                .get(column.name.as_str())
                .and_then(|found| found.get(column.occurrence).copied())
        })
        .collect()
}

/// Write the header record. Duplicate columns repeat their name (`id,id`),
/// which is what the table renderer prints and what keeps the record arity
/// equal to the number of columns actually emitted.
pub(crate) fn write_csv_header(
    writer: &mut dyn Write,
    columns: &[CsvColumn],
) -> anyhow::Result<()> {
    if columns.is_empty() {
        return Ok(());
    }
    let names: Vec<&str> = columns.iter().map(|c| c.name.as_str()).collect();
    writer.write_all(&csv_record_bytes(&names)?)?;
    Ok(())
}

/// Write every row of `batch` as a CSV record, aligned to `columns`.
/// Returns the number of rows written.
///
/// Cells are rendered with Arrow's `ArrayFormatter` — the same formatter the
/// table renderer uses — so `-f csv` and `-f table` agree cell for cell, and
/// so the rendering is *positional*. The previous file-side implementations
/// went through `batch_to_json_rows`, which builds a map keyed by field
/// name and therefore cannot represent two columns of the same name at all.
pub(crate) fn write_batch_csv_rows(
    writer: &mut dyn Write,
    columns: &[CsvColumn],
    batch: &RecordBatch,
) -> anyhow::Result<usize> {
    let schema = batch.schema();
    let indices = column_indices(columns, schema.as_ref());
    let options = FormatOptions::default();
    let formatters: Vec<Option<ArrayFormatter>> = indices
        .iter()
        .zip(columns)
        .map(|(index, column)| match index {
            Some(i) => ArrayFormatter::try_new(batch.column(*i).as_ref(), &options)
                .map(Some)
                .map_err(|e| anyhow::anyhow!("cannot render column '{}' as CSV: {e}", column.name)),
            None => Ok(None),
        })
        .collect::<anyhow::Result<_>>()?;

    for row_idx in 0..batch.num_rows() {
        let cells: Vec<String> = formatters
            .iter()
            .map(|f| match f {
                Some(fmt) => fmt.value(row_idx).to_string(),
                // A column this batch's file doesn't have: empty, never
                // omitted, so the record never goes ragged.
                None => String::new(),
            })
            .collect();
        writer.write_all(&csv_record_bytes(&cells)?)?;
    }
    Ok(batch.num_rows())
}

/// Render a whole in-memory batch set as CSV. Returns the number of rows.
pub(crate) fn write_batches_csv(
    writer: &mut dyn Write,
    batches: &[RecordBatch],
) -> anyhow::Result<usize> {
    let columns = union_columns(batches.iter().map(|b| b.schema()));
    write_csv_header(writer, &columns)?;
    let mut rows = 0;
    for batch in batches {
        rows += write_batch_csv_rows(writer, &columns, batch)?;
    }
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Values-derived CSV (the jq path). Here the input genuinely *is* a map of
// names to values — jq can add, rename, or drop fields per row and there is
// no Arrow schema to consult — so lookup stays keyed by name. A JSON object
// cannot have duplicate keys, so the duplicate-column problem does not
// arise on this path.
// ---------------------------------------------------------------------------

/// Header for jq output: the union of the values' own keys, first-seen
/// order.
fn union_header_from_values(values: &[serde_json::Value]) -> Vec<String> {
    let mut header = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        if let Some(obj) = value.as_object() {
            for key in obj.keys() {
                if seen.insert(key.clone()) {
                    header.push(key.clone());
                }
            }
        }
    }
    header
}

/// A row's value for one header column: empty for a key this row's object
/// doesn't have (or a JSON null), so a column absent from one row never
/// causes it to be dropped or to appear under a different column.
fn csv_cell(value: Option<&serde_json::Value>) -> String {
    match value {
        None | Some(serde_json::Value::Null) => String::new(),
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

/// Build one CSV record from an object row, keyed by column name against
/// `header` — never by positional/iteration order, which is what let a
/// `val` land under `name` in the original bug.
fn csv_record(header: &[String], obj: &serde_json::Map<String, serde_json::Value>) -> Vec<String> {
    header.iter().map(|k| csv_cell(obj.get(k))).collect()
}

/// Render one CSV record (with correct quoting, via the `csv` crate) to a
/// byte buffer. Writing record-by-record into a scratch buffer, rather than
/// holding one long-lived `csv::Writer` over the whole output, lets the
/// caller freely interleave non-CSV raw writes (see `write_values_csv`'s
/// non-object fallback) without fighting the writer's ownership of the
/// underlying `dyn Write`.
///
/// Uses the `csv` crate rather than hand-rolled quoting: a bare `\r` is just
/// as much a record separator to a compliant CSV reader as `\n` or `\r\n`,
/// but a hand-rolled check that only tests for `,`, `"`, and `\n` leaves a
/// lone `\r` unquoted, silently splitting one row into two on read.
pub(crate) fn csv_record_bytes<T: AsRef<str>>(fields: &[T]) -> anyhow::Result<Vec<u8>> {
    let mut wtr = csv::WriterBuilder::new().from_writer(Vec::new());
    wtr.write_record(fields.iter().map(|f| f.as_ref()))?;
    wtr.into_inner()
        .map_err(|e| anyhow::anyhow!("failed to flush CSV record: {e}"))
}

fn write_values_csv(writer: &mut dyn Write, values: &[serde_json::Value]) -> anyhow::Result<()> {
    let header = union_header_from_values(values);
    if !header.is_empty() {
        writer.write_all(&csv_record_bytes(&header)?)?;
    }
    for value in values {
        match value.as_object() {
            Some(obj) => {
                writer.write_all(&csv_record_bytes(&csv_record(&header, obj))?)?;
            }
            None => {
                // Non-object jq output (a bare scalar) doesn't fit the
                // column model; preserve the pre-existing fallback of
                // emitting it as a raw JSON line rather than as a CSV
                // record.
                serde_json::to_writer(&mut *writer, value)?;
                writeln!(writer)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::{ArrayRef, Int64Array, StringArray};
    use arrow::datatypes::{DataType, Field};
    use std::sync::Arc;

    /// A `Write` that accepts every `write` (so data can build up in the
    /// `BufWriter` in front of it without erroring) but fails `flush` — the
    /// shape of a full disk, a broken pipe noticed late, or any other
    /// backend failure that only surfaces when the buffered bytes are
    /// finally pushed out.
    struct FailsOnFlush;

    impl Write for FailsOnFlush {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("simulated flush failure"))
        }
    }

    #[test]
    fn write_buffered_propagates_a_flush_error() {
        // This is the exact mechanism `write_batches_as` and
        // `json_values_to_file` rely on to keep `with_atomic_output` from
        // renaming a staged file that never made it fully to disk. If a
        // future edit changes `write_buffered`'s `buffered.flush()?` to
        // something that swallows the error (`let _ = buffered.flush();`),
        // this test fails: `result` becomes `Ok(())` instead of `Err`.
        let result: anyhow::Result<()> = write_buffered(FailsOnFlush, |w| {
            w.write_all(b"some bytes that only live in the BufWriter until flush")?;
            Ok(())
        });
        assert!(
            result.is_err(),
            "write_buffered must surface a flush failure, not swallow it"
        );
    }

    #[test]
    fn write_buffered_flushes_through_to_the_inner_writer_on_success() {
        // Written through a real file rather than an in-memory buffer so
        // that "the bytes actually reached the backing store" is a
        // meaningful check: reading the file back only sees what was
        // flushed, not merely what `write_all` staged in the `BufWriter`.
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("out.txt");
        let n = write_buffered(std::fs::File::create(&path).unwrap(), |w| {
            w.write_all(b"hello")?;
            Ok(5usize)
        })
        .unwrap();
        assert_eq!(n, 5);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
    }

    fn dup_id_batch() -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("id", DataType::Int64, false),
        ]));
        let first: ArrayRef = Arc::new(Int64Array::from(vec![1_i64, 2]));
        let second: ArrayRef = Arc::new(Int64Array::from(vec![10_i64, 20]));
        RecordBatch::try_new(schema, vec![first, second]).unwrap()
    }

    fn render(batches: &[RecordBatch]) -> String {
        let mut buf: Vec<u8> = Vec::new();
        write_batches_csv(&mut buf, batches).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn duplicate_names_get_one_column_each_resolved_positionally() {
        // `Schema::index_of("id")` returns 0 for both fields, which is how
        // the second column used to disappear.
        assert_eq!(render(&[dup_id_batch()]), "id,id\n1,10\n2,20\n");
    }

    #[test]
    fn duplicate_names_are_counted_per_schema_not_globally() {
        // Two batches with the same duplicate-name schema must union to two
        // columns, not four.
        let columns = union_columns([dup_id_batch().schema(), dup_id_batch().schema()]);
        assert_eq!(columns.len(), 2);
        assert_eq!(
            render(&[dup_id_batch(), dup_id_batch()]),
            "id,id\n1,10\n2,20\n1,10\n2,20\n"
        );
    }

    #[test]
    fn heterogeneous_schemas_still_union_by_name() {
        let a = RecordBatch::try_new(
            Arc::new(Schema::new(vec![
                Field::new("id", DataType::Int64, false),
                Field::new("name", DataType::Utf8, false),
            ])),
            vec![
                Arc::new(Int64Array::from(vec![1_i64])) as ArrayRef,
                Arc::new(StringArray::from(vec!["alice"])) as ArrayRef,
            ],
        )
        .unwrap();
        let b = RecordBatch::try_new(
            Arc::new(Schema::new(vec![
                Field::new("id", DataType::Int64, false),
                Field::new("val", DataType::Int64, false),
            ])),
            vec![
                Arc::new(Int64Array::from(vec![3_i64])) as ArrayRef,
                Arc::new(Int64Array::from(vec![30_i64])) as ArrayRef,
            ],
        )
        .unwrap();
        // `val` must appear as its own column and `name` must be blank for
        // the row that has no `name` — not shifted, not dropped.
        assert_eq!(render(&[a, b]), "id,name,val\n1,alice,\n3,,30\n");
    }

    #[test]
    fn format_is_taken_from_the_argument_not_the_path() {
        // The `sql -o` defect at the unit level: a `.csv` path asked to
        // write Parquet must produce Parquet, never re-sniff its own name.
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("staged.csv");
        write_batches_as(&path, &[dup_id_batch()], OutputFileFormat::Parquet).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(
            &bytes[..4],
            b"PAR1",
            "write_batches_as re-sniffed the path instead of obeying its format argument"
        );
    }
}
