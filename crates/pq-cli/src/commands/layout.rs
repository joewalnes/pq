use std::io::Write;
use std::path::Path;

use pq_core::metadata::read_metadata;
use pq_core::physical_layout::extract_physical_layout;

use crate::output::Format;

pub fn run(file: &str, format: Format) -> anyhow::Result<()> {
    let path = Path::new(file);
    let metadata = read_metadata(path)?;
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

            for rg in &layout.row_groups {
                writeln!(
                    writer,
                    "Row Group {}: {} rows, {}",
                    rg.index,
                    rg.num_rows,
                    bytesize::ByteSize(rg.total_byte_size as u64),
                )?;

                for col in &rg.columns {
                    writeln!(
                        writer,
                        "  {} ({}) [{:}] compressed={} uncompressed={} values={}{}",
                        col.path,
                        col.physical_type,
                        col.compression,
                        bytesize::ByteSize(col.compressed_size as u64),
                        bytesize::ByteSize(col.uncompressed_size as u64),
                        col.num_values,
                        if col.has_bloom_filter { " [bloom]" } else { "" },
                    )?;
                }
                writeln!(writer)?;
            }
        }
    }

    Ok(())
}
