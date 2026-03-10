use std::io::Write;

use crate::output::Format;

pub fn run(
    files: &[String],
    output: &str,
    format: Option<Format>,
) -> anyhow::Result<()> {
    let format = format.unwrap_or_else(|| {
        // Auto-detect from output extension
        match std::path::Path::new(output)
            .extension()
            .and_then(|e| e.to_str())
        {
            Some("json") => Format::Json,
            Some("jsonl" | "ndjson") => Format::JsonLines,
            Some("csv") => Format::Csv,
            _ => Format::JsonLines,
        }
    });

    let opts = pq_core::reader::ReadOptions::default();

    let mut out_file = std::fs::File::create(output)?;
    let mut total_rows: usize = 0;
    let mut wrote_csv_header = false;

    // For JSON array format, collect all rows first
    if format == Format::Json {
        let mut all_rows: Vec<serde_json::Value> = Vec::new();
        for f in files {
            let (batches, _schema) = pq_core::reader::open_batches(f, &opts)?;
            for batch in &batches {
                let rows = pq_query::convert::batch_to_json_rows(batch);
                total_rows += rows.len();
                all_rows.extend(rows);
            }
        }
        serde_json::to_writer_pretty(&mut out_file, &all_rows)?;
        writeln!(out_file)?;
    } else {
        for f in files {
            let (batches, _schema) = pq_core::reader::open_batches(f, &opts)?;
            match format {
                Format::JsonLines | Format::Plain => {
                    for batch in &batches {
                        let rows = pq_query::convert::batch_to_json_rows(batch);
                        total_rows += rows.len();
                        for row in &rows {
                            serde_json::to_writer(&mut out_file, row)?;
                            writeln!(out_file)?;
                        }
                    }
                }
                Format::Csv => {
                    for batch in &batches {
                        let rows = pq_query::convert::batch_to_json_rows(batch);
                        if rows.is_empty() {
                            continue;
                        }
                        // Write header from first row
                        if !wrote_csv_header {
                            if let Some(obj) = rows[0].as_object() {
                                let keys: Vec<&str> =
                                    obj.keys().map(|k| k.as_str()).collect();
                                writeln!(out_file, "{}", keys.join(","))?;
                                wrote_csv_header = true;
                            }
                        }
                        total_rows += rows.len();
                        for row in &rows {
                            if let Some(obj) = row.as_object() {
                                let vals: Vec<String> = obj
                                    .values()
                                    .map(|v| match v {
                                        serde_json::Value::String(s) => {
                                            if s.contains(',') || s.contains('"') || s.contains('\n') {
                                                format!("\"{}\"", s.replace('"', "\"\""))
                                            } else {
                                                s.clone()
                                            }
                                        }
                                        serde_json::Value::Null => String::new(),
                                        other => other.to_string(),
                                    })
                                    .collect();
                                writeln!(out_file, "{}", vals.join(","))?;
                            }
                        }
                    }
                }
                _ => unreachable!(),
            }
        }
    }

    eprintln!("Exported {total_rows} rows to {output}");
    Ok(())
}
