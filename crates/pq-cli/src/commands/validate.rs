use std::io::Write;

use crate::output::Format;

#[derive(Debug, serde::Serialize)]
pub struct ValidationResult {
    pub file: String,
    pub valid: bool,
    pub num_rows: i64,
    pub num_row_groups: usize,
    pub num_columns: usize,
    pub issues: Vec<String>,
}

pub fn run(file: &str, format: Format) -> anyhow::Result<()> {
    let mut issues: Vec<String> = Vec::new();
    let mut valid = true;

    // 1. Check metadata can be read
    let metadata = match pq_core::metadata::open_metadata(file) {
        Ok(m) => m,
        Err(e) => {
            let result = ValidationResult {
                file: file.to_string(),
                valid: false,
                num_rows: 0,
                num_row_groups: 0,
                num_columns: 0,
                issues: vec![format!("Cannot read file metadata: {e}")],
            };
            return print_result(&result, format);
        }
    };

    let file_meta = metadata.file_metadata();
    let num_rows = file_meta.num_rows();
    let num_row_groups = metadata.num_row_groups();
    let num_columns = file_meta.schema_descr().num_columns();

    // 2. Check row count consistency across row groups
    let rg_row_sum: i64 = metadata.row_groups().iter().map(|rg| rg.num_rows()).sum();
    if rg_row_sum != num_rows {
        issues.push(format!(
            "Row count mismatch: file metadata says {num_rows} but row groups sum to {rg_row_sum}"
        ));
        valid = false;
    }

    // 3. Check each row group has the expected number of columns
    for (i, rg) in metadata.row_groups().iter().enumerate() {
        if rg.columns().len() != num_columns {
            issues.push(format!(
                "Row group {i}: expected {num_columns} columns but found {}",
                rg.columns().len()
            ));
            valid = false;
        }
    }

    // 4. Check statistics sanity
    for (i, rg) in metadata.row_groups().iter().enumerate() {
        for (j, col) in rg.columns().iter().enumerate() {
            if let Some(stats) = col.statistics() {
                if let Some(null_count) = stats.null_count_opt() {
                    if null_count > col.num_values() as u64 {
                        issues.push(format!(
                            "Row group {i}, column {j}: null_count ({null_count}) > num_values ({})",
                            col.num_values()
                        ));
                        valid = false;
                    }
                }
            }

            // Check compressed vs uncompressed sizes are reasonable
            if col.compressed_size() > col.uncompressed_size() * 2 {
                // Compressed larger than 2x uncompressed is suspicious but not necessarily invalid
                issues.push(format!(
                    "Row group {i}, column {j}: compressed size ({}) > 2x uncompressed size ({}); possible metadata error",
                    col.compressed_size(),
                    col.uncompressed_size()
                ));
                // Don't fail for this — it's a warning
            }
        }
    }

    // 5. Try to actually read data (schema validation)
    let opts = pq_core::reader::ReadOptions {
        limit: Some(1),
        ..Default::default()
    };
    match pq_core::reader::open_batches(file, &opts) {
        Ok(_) => {}
        Err(e) => {
            issues.push(format!("Cannot read data: {e}"));
            valid = false;
        }
    }

    // 6. Check for empty file
    if num_rows == 0 && num_row_groups == 0 {
        issues.push("File contains no data (0 rows, 0 row groups)".to_string());
    }

    let result = ValidationResult {
        file: file.to_string(),
        valid,
        num_rows,
        num_row_groups,
        num_columns,
        issues,
    };

    print_result(&result, format)
}

fn print_result(result: &ValidationResult, format: Format) -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    match format {
        Format::Json | Format::JsonLines => {
            let json = serde_json::to_value(result)?;
            crate::output::render_value(&mut writer, &json, format)?;
        }
        Format::Table => {
            let status = if result.valid { "VALID" } else { "INVALID" };
            writeln!(writer, "{}: {status}", result.file)?;
            writeln!(
                writer,
                "  Rows: {}  Row groups: {}  Columns: {}",
                result.num_rows, result.num_row_groups, result.num_columns
            )?;
            if result.issues.is_empty() {
                writeln!(writer, "  No issues found")?;
            } else {
                writeln!(writer, "  Issues ({}):", result.issues.len())?;
                for issue in &result.issues {
                    writeln!(writer, "    - {issue}")?;
                }
            }
        }
        _ => {
            let status = if result.valid { "VALID" } else { "INVALID" };
            writeln!(writer, "{}\t{status}", result.file)?;
            for issue in &result.issues {
                writeln!(writer, "  {issue}")?;
            }
        }
    }

    if !result.valid {
        std::process::exit(1);
    }

    Ok(())
}
