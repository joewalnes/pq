use std::io::Write;

use pq_core::metadata::open_file_metadata;

use crate::output::Format;

pub fn run(file: &str, format: Format) -> anyhow::Result<()> {
    let meta = open_file_metadata(file)?;

    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    match format {
        Format::Json | Format::JsonLines => {
            let json = serde_json::to_value(&meta)?;
            crate::output::render_value(&mut writer, &json, format)?;
        }
        _ => {
            writeln!(writer, "File:         {}", meta.path)?;
            writeln!(
                writer,
                "Size:         {}",
                bytesize::ByteSize(meta.file_size)
            )?;
            writeln!(writer, "Rows:         {}", meta.num_rows)?;
            writeln!(writer, "Row Groups:   {}", meta.num_row_groups)?;
            writeln!(writer, "Columns:      {}", meta.num_columns)?;
            writeln!(writer, "Format:       v{}", meta.format_version)?;
            if let Some(ref created_by) = meta.created_by {
                writeln!(writer, "Created by:   {created_by}")?;
            }
            writeln!(writer, "Compression:  {}", meta.compression.join(", "))?;
            if !meta.key_value_metadata.is_empty() {
                writeln!(writer, "Metadata:")?;
                for kv in &meta.key_value_metadata {
                    let val = kv.value.as_deref().unwrap_or("<none>");
                    let display_val = if val.len() > 100 {
                        format!("{}...", &val[..100])
                    } else {
                        val.to_string()
                    };
                    writeln!(writer, "  {}: {display_val}", kv.key)?;
                }
            }
        }
    }

    Ok(())
}
