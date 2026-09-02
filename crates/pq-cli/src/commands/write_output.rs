use std::io::Write;
use std::path::Path;

use arrow::array::RecordBatch;

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
        OutputFileFormat::Csv => {
            let mut wrote_header = false;
            for batch in batches {
                let rows = pq_query::convert::batch_to_json_rows(batch);
                if !wrote_header {
                    if let Some(obj) = rows.first().and_then(|r| r.as_object()) {
                        let keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
                        writeln!(writer, "{}", keys.join(","))?;
                        wrote_header = true;
                    }
                }
                for row in &rows {
                    if let Some(obj) = row.as_object() {
                        let vals: Vec<String> = obj.values().map(|v| csv_escape(v)).collect();
                        writeln!(writer, "{}", vals.join(","))?;
                    }
                }
            }
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
        OutputFileFormat::Csv => {
            // Collect headers from first object value
            let mut wrote_header = false;
            for value in values {
                if let Some(obj) = value.as_object() {
                    if !wrote_header {
                        let keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
                        writeln!(writer, "{}", keys.join(","))?;
                        wrote_header = true;
                    }
                    let vals: Vec<String> = obj.values().map(|v| csv_escape(v)).collect();
                    writeln!(writer, "{}", vals.join(","))?;
                } else {
                    // Non-object values: write as single column
                    serde_json::to_writer(&mut *writer, value)?;
                    writeln!(writer)?;
                }
            }
        }
        OutputFileFormat::Parquet => unreachable!(),
    }
    Ok(())
}

fn csv_escape(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => {
            if s.contains(',') || s.contains('"') || s.contains('\n') {
                format!("\"{}\"", s.replace('"', "\"\""))
            } else {
                s.clone()
            }
        }
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}
