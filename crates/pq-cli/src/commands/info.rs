use std::io::Write;

use pq_core::metadata::open_file_metadata;

use crate::output::Format;

/// Format a number with comma separators (e.g., 1234567 → "1,234,567").
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
            writeln!(writer, "Rows:         {}", format_number(meta.num_rows))?;
            writeln!(
                writer,
                "Row Groups:   {}",
                format_number(meta.num_row_groups)
            )?;
            writeln!(writer, "Columns:      {}", format_number(meta.num_columns))?;
            writeln!(writer, "Format:       v{}", meta.format_version)?;
            if let Some(ref created_by) = meta.created_by {
                writeln!(writer, "Created by:   {created_by}")?;
            }
            writeln!(writer, "Compression:  {}", meta.compression.join(", "))?;
            if !meta.key_value_metadata.is_empty() {
                writeln!(writer, "Metadata:")?;
                for kv in &meta.key_value_metadata {
                    let val = kv.value.as_deref().unwrap_or("<none>");

                    if kv.key == "ARROW:schema" {
                        // Decode the Arrow IPC schema from base64
                        writeln!(writer, "  {}:", kv.key)?;
                        if let Some(decoded) = decode_arrow_schema(val) {
                            for line in decoded.lines() {
                                writeln!(writer, "    {line}")?;
                            }
                        } else {
                            writeln!(
                                writer,
                                "    (base64-encoded Arrow IPC schema, {} bytes)",
                                val.len()
                            )?;
                        }
                    } else if looks_like_json(val) {
                        // Pretty-print JSON metadata values
                        writeln!(writer, "  {}:", kv.key)?;
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(val) {
                            let pretty = serde_json::to_string_pretty(&parsed)
                                .unwrap_or_else(|_| val.to_string());
                            for line in pretty.lines() {
                                writeln!(writer, "    {line}")?;
                            }
                        } else {
                            for line in val.lines() {
                                writeln!(writer, "    {line}")?;
                            }
                        }
                    } else {
                        // Show full value with continuation indent for long values
                        writeln!(writer, "  {}: {val}", kv.key)?;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Try to decode an ARROW:schema base64 value into field names and types.
fn decode_arrow_schema(base64_val: &str) -> Option<String> {
    let bytes = base64_decode(base64_val)?;

    // Use the Arrow IPC decoder which handles both the old format (length prefix only)
    // and the new format (continuation marker + length prefix + flatbuffer).
    let schema = arrow::ipc::convert::try_schema_from_ipc_buffer(&bytes).ok()?;

    let mut result = String::new();
    for field in schema.fields() {
        let type_name = pq_core::schema::format_data_type_public(field.data_type());
        let nullable = if field.is_nullable() {
            " (nullable)"
        } else {
            ""
        };
        result.push_str(&format!("{}: {type_name}{nullable}\n", field.name()));
    }

    if result.is_empty() {
        None
    } else {
        Some(result.trim_end().to_string())
    }
}

fn base64_decode(input: &str) -> Option<Vec<u8>> {
    const DECODE_TABLE: [u8; 128] = {
        let mut table = [255u8; 128];
        let mut i = 0u8;
        while i < 26 {
            table[(b'A' + i) as usize] = i;
            table[(b'a' + i) as usize] = i + 26;
            i += 1;
        }
        let mut i = 0u8;
        while i < 10 {
            table[(b'0' + i) as usize] = i + 52;
            i += 1;
        }
        table[b'+' as usize] = 62;
        table[b'/' as usize] = 63;
        table
    };

    let input = input.trim();
    let mut bytes = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;

    for &b in input.as_bytes() {
        if b == b'=' || b == b'\n' || b == b'\r' || b == b' ' {
            continue;
        }
        if b >= 128 {
            return None;
        }
        let val = DECODE_TABLE[b as usize];
        if val == 255 {
            return None;
        }
        buf = (buf << 6) | val as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }

    Some(bytes)
}

fn looks_like_json(s: &str) -> bool {
    let trimmed = s.trim();
    (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
}
