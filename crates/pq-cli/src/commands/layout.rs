use std::io::Write;

use pq_core::metadata::open_metadata;
use pq_core::physical_layout::extract_physical_layout;

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
    let layout = extract_physical_layout(&metadata);

    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    match format {
        Format::Json | Format::JsonLines => {
            let json = serde_json::to_value(&layout)?;
            crate::output::render_value(&mut writer, &json, format)?;
        }
        _ => {
            writeln!(
                writer,
                "Physical Layout: {} row groups",
                layout.num_row_groups
            )?;
            writeln!(writer)?;

            let mut row_offset: i64 = 0;
            for rg in &layout.row_groups {
                let row_start = row_offset;
                let row_end = row_start + rg.num_rows.saturating_sub(1).max(0);
                row_offset += rg.num_rows;
                writeln!(
                    writer,
                    "Row Group {} (rows {}\u{2013}{}, {}):",
                    rg.index,
                    format_number(row_start),
                    format_number(row_end),
                    bytesize::ByteSize(rg.total_byte_size as u64),
                )?;

                // Compute column widths for alignment
                let mut max_path = 6; // "Column"
                let mut max_type = 4; // "Type"
                let mut max_codec = 5; // "Codec"
                let mut max_values = 6; // "Values"
                for col in &rg.columns {
                    max_path = max_path.max(col.path.len());
                    max_type = max_type.max(col.physical_type.len());
                    max_codec = max_codec.max(col.compression.len());
                    max_values = max_values.max(format_number(col.num_values).len());
                }

                writeln!(
                    writer,
                    "  {:max_path$}  {:max_type$}  {:max_codec$}  {:>12}  {:>14}  {:>max_values$}  Bytes",
                    "Column", "Type", "Codec", "Compressed", "Uncompressed", "Values",
                )?;

                for col in &rg.columns {
                    // A dictionary page, when present, precedes the data page
                    // in the column chunk and is included in
                    // `compressed_size` — so the chunk's true start is the
                    // dictionary page offset, not the data page offset.
                    let byte_start = col.dictionary_page_offset.unwrap_or(col.data_page_offset);
                    let byte_end = byte_start + col.compressed_size;
                    writeln!(
                        writer,
                        "  {:max_path$}  {:max_type$}  {:max_codec$}  {:>12}  {:>14}  {:>max_values$}  {}\u{2013}{}{}",
                        col.path,
                        col.physical_type,
                        col.compression,
                        format_number(col.compressed_size),
                        format_number(col.uncompressed_size),
                        format_number(col.num_values),
                        format_number(byte_start),
                        format_number(byte_end),
                        if col.has_bloom_filter { " [bloom]" } else { "" },
                    )?;
                }
                writeln!(writer)?;
            }
        }
    }

    Ok(())
}
