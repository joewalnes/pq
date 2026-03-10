use std::io::Write;

use arrow::array::RecordBatch;
use regex::Regex;

use crate::output::Format;

pub fn run(
    files: &[String],
    pattern: &str,
    columns: Option<Vec<String>>,
    limit: Option<usize>,
    ignore_case: bool,
    format: Format,
) -> anyhow::Result<()> {
    let regex_pattern = if ignore_case {
        format!("(?i){pattern}")
    } else {
        pattern.to_string()
    };
    let re = Regex::new(&regex_pattern)
        .map_err(|e| anyhow::anyhow!("Invalid regex pattern '{}': {}", pattern, e))?;

    let opts = pq_core::reader::ReadOptions {
        columns: columns.clone(),
        ..Default::default()
    };

    let mut matched_rows: Vec<serde_json::Value> = Vec::new();
    let mut remaining = limit;

    'outer: for file in files {
        let (batches, _schema) = pq_core::reader::open_batches(file, &opts)?;
        for batch in &batches {
            let matching = grep_batch(batch, &re)?;
            for row in matching {
                matched_rows.push(row);
                if let Some(ref mut rem) = remaining {
                    *rem = rem.saturating_sub(1);
                    if *rem == 0 {
                        break 'outer;
                    }
                }
            }
        }
    }

    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    match format {
        Format::Json => {
            serde_json::to_writer_pretty(&mut writer, &matched_rows)?;
            writeln!(writer)?;
        }
        Format::Table => {
            // Convert back to record batches for table rendering
            if matched_rows.is_empty() {
                writeln!(writer, "No matches found")?;
            } else {
                for row in &matched_rows {
                    serde_json::to_writer(&mut writer, row)?;
                    writeln!(writer)?;
                }
            }
        }
        _ => {
            for row in &matched_rows {
                serde_json::to_writer(&mut writer, row)?;
                writeln!(writer)?;
            }
        }
    }

    if matched_rows.is_empty() {
        std::process::exit(1);
    }

    Ok(())
}

fn grep_batch(batch: &RecordBatch, re: &Regex) -> anyhow::Result<Vec<serde_json::Value>> {
    let rows = pq_query::convert::batch_to_json_rows(batch);
    let mut matched = Vec::new();

    for row in rows {
        if row_matches(&row, re) {
            matched.push(row);
        }
    }

    Ok(matched)
}

fn row_matches(value: &serde_json::Value, re: &Regex) -> bool {
    match value {
        serde_json::Value::String(s) => re.is_match(s),
        serde_json::Value::Number(n) => re.is_match(&n.to_string()),
        serde_json::Value::Bool(b) => re.is_match(&b.to_string()),
        serde_json::Value::Array(arr) => arr.iter().any(|v| row_matches(v, re)),
        serde_json::Value::Object(obj) => obj.values().any(|v| row_matches(v, re)),
        serde_json::Value::Null => false,
    }
}
