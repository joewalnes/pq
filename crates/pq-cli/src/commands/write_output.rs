use std::collections::HashSet;
use std::io::Write;
use std::path::Path;

use arrow::array::RecordBatch;
use arrow::datatypes::SchemaRef;

/// Output file format, auto-detected from extension.
pub enum OutputFileFormat {
    Parquet,
    Json,
    JsonLines,
    Csv,
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

/// Write RecordBatches to a file, auto-detecting format from extension.
/// Returns the number of rows written.
pub fn write_batches_to_file(path: &str, batches: &[RecordBatch]) -> anyhow::Result<usize> {
    let total_rows: usize = batches.iter().map(|b| b.num_rows()).sum();

    match format_from_extension(path) {
        OutputFileFormat::Parquet => {
            if batches.is_empty() {
                anyhow::bail!("No data to write");
            }
            let opts = pq_core::writer::WriteOptions::default();
            pq_core::writer::write_batches(Path::new(path), batches, &opts)?;
        }
        format => {
            let mut file = std::fs::File::create(path)?;
            write_batches_text(&mut file, batches, format)?;
        }
    }

    Ok(total_rows)
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
            let mut file = std::fs::File::create(path)?;
            write_values_text(&mut file, values, format)?;
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
        OutputFileFormat::Csv => write_batches_csv(writer, batches)?,
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

/// Header for a batch-derived CSV: the union of every batch's schema field
/// names, in first-seen order — not just the first batch's.
///
/// `pq cat a.parquet b.parquet --output out.csv` combines files that can
/// have different schemas with no per-row key lookup on the naive approach:
/// a header frozen from batch 0 either shifts a later batch's values under
/// the wrong column name (if key sets merely differ) or silently drops a
/// column batch 0 didn't have. Dropping a value that the user has but that
/// never reaches the output is the same class of bug as shifting it.
///
/// `batches` is already fully resident in memory by the time this runs (the
/// caller collected every batch before calling in), so building the union
/// costs one extra pass over already-known field lists, not extra
/// buffering. See `union_header_from_values` for the analogous, non-schema
/// case.
pub(crate) fn union_header(schemas: impl IntoIterator<Item = SchemaRef>) -> Vec<String> {
    let mut header = Vec::new();
    let mut seen = HashSet::new();
    for schema in schemas {
        for field in schema.fields() {
            if seen.insert(field.name().clone()) {
                header.push(field.name().clone());
            }
        }
    }
    header
}

/// Same idea as `union_header`, but for jq output: there is no Arrow schema
/// to consult (jq can add, rename, or drop fields per row), so the union is
/// computed from the values' own keys instead, in first-seen order.
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
/// doesn't have (or a JSON null), so a column absent from one file/row never
/// causes it to be dropped or to appear at all under a different column.
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
pub(crate) fn csv_record(
    header: &[String],
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Vec<String> {
    header.iter().map(|k| csv_cell(obj.get(k))).collect()
}

/// Render one CSV record (with correct quoting, via the `csv` crate) to a
/// byte buffer. Writing record-by-record into a scratch buffer, rather than
/// holding one long-lived `csv::Writer` over the whole output, lets the
/// caller freely interleave non-CSV raw writes (see `write_values_csv`'s
/// non-object fallback) without fighting the writer's ownership of the
/// underlying `dyn Write`.
pub(crate) fn csv_record_bytes<T: AsRef<str>>(fields: &[T]) -> anyhow::Result<Vec<u8>> {
    let mut wtr = csv::WriterBuilder::new().from_writer(Vec::new());
    wtr.write_record(fields.iter().map(|f| f.as_ref()))?;
    wtr.into_inner()
        .map_err(|e| anyhow::anyhow!("failed to flush CSV record: {e}"))
}

fn write_batches_csv(writer: &mut dyn Write, batches: &[RecordBatch]) -> anyhow::Result<()> {
    let header = union_header(batches.iter().map(|b| b.schema()));
    if !header.is_empty() {
        writer.write_all(&csv_record_bytes(&header)?)?;
    }
    for batch in batches {
        for row in pq_query::convert::batch_to_json_rows(batch) {
            if let Some(obj) = row.as_object() {
                writer.write_all(&csv_record_bytes(&csv_record(&header, obj))?)?;
            }
        }
    }
    Ok(())
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
