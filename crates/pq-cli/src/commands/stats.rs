use std::io::Write;

use pq_core::metadata::open_metadata;
use pq_core::statistics::extract_column_stats;

use crate::output::Format;

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
                modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Cell, ContentArrangement, Table,
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
                    Cell::new(s.num_values),
                    Cell::new(
                        s.null_count
                            .map(|n| n.to_string())
                            .unwrap_or("-".to_string()),
                    ),
                    Cell::new(
                        s.distinct_count
                            .map(|n| n.to_string())
                            .unwrap_or("-".to_string()),
                    ),
                    Cell::new(s.min_value.as_deref().unwrap_or("-")),
                    Cell::new(s.max_value.as_deref().unwrap_or("-")),
                    Cell::new(bytesize::ByteSize(s.compressed_size as u64).to_string()),
                    Cell::new(bytesize::ByteSize(s.uncompressed_size as u64).to_string()),
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
