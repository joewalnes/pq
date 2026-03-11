use std::io::Write;

use pq_core::metadata::open_metadata;
use pq_core::statistics::extract_column_stats;

use crate::output::Format;

/// Format a number with comma separators.
fn format_number(n: impl std::fmt::Display) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, &b) in bytes.iter().rev().enumerate() {
        if i > 0 && i % 3 == 0 && b != b'-' {
            result.insert(0, ',');
        }
        result.insert(0, b as char);
    }
    result
}

pub fn run(file: &str, format: Format) -> anyhow::Result<()> {
    let metadata = open_metadata(file)?;
    let stats = extract_column_stats(&metadata);

    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    match format {
        Format::Json | Format::JsonLines => {
            let json = serde_json::to_value(&stats)?;
            crate::output::render_value(&mut writer, &json, format)?;
        }
        Format::Table => {
            use comfy_table::{
                modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Cell, CellAlignment,
                ContentArrangement, Table,
            };
            let mut table = Table::new();
            table
                .load_preset(UTF8_FULL)
                .apply_modifier(UTF8_ROUND_CORNERS)
                .set_content_arrangement(ContentArrangement::Dynamic);

            table.set_header(vec![
                "Column",
                "Type",
                "Values",
                "Nulls",
                "Distinct",
                "Min",
                "Max",
                "Compressed",
                "Uncompressed",
            ]);

            for s in &stats {
                table.add_row(vec![
                    Cell::new(&s.column_name),
                    Cell::new(&s.column_type),
                    Cell::new(format_number(s.num_values)).set_alignment(CellAlignment::Right),
                    Cell::new(s.null_count.map(format_number).unwrap_or("-".to_string()))
                        .set_alignment(CellAlignment::Right),
                    Cell::new(
                        s.distinct_count
                            .map(format_number)
                            .unwrap_or("-".to_string()),
                    )
                    .set_alignment(CellAlignment::Right),
                    Cell::new(s.min_value.as_deref().unwrap_or("-"))
                        .set_alignment(CellAlignment::Right),
                    Cell::new(s.max_value.as_deref().unwrap_or("-"))
                        .set_alignment(CellAlignment::Right),
                    Cell::new(bytesize::ByteSize(s.compressed_size as u64).to_string())
                        .set_alignment(CellAlignment::Right),
                    Cell::new(bytesize::ByteSize(s.uncompressed_size as u64).to_string())
                        .set_alignment(CellAlignment::Right),
                ]);
            }

            writeln!(writer, "{table}")?;
        }
        _ => {
            for s in &stats {
                writeln!(
                    writer,
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    s.column_name,
                    s.column_type,
                    s.num_values,
                    s.null_count
                        .map(|n| n.to_string())
                        .unwrap_or("-".to_string()),
                    s.min_value.as_deref().unwrap_or("-"),
                    s.max_value.as_deref().unwrap_or("-"),
                    bytesize::ByteSize(s.compressed_size as u64),
                )?;
            }
        }
    }

    Ok(())
}
