use std::io::Write;
use std::path::Path;

use pq_core::reader::row_count;

use crate::output::Format;

pub fn run(files: &[String], format: Format) -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    let mut results: Vec<(String, i64)> = Vec::new();
    let mut total: i64 = 0;

    for file in files {
        let path = Path::new(file);
        let count = row_count(path)?;
        total += count;
        results.push((file.clone(), count));
    }

    match format {
        Format::Json | Format::JsonLines => {
            if files.len() == 1 {
                let json = serde_json::json!({
                    "file": results[0].0,
                    "count": results[0].1,
                });
                crate::output::render_value(&mut writer, &json, format)?;
            } else {
                let json = serde_json::json!({
                    "files": results.iter().map(|(f, c)| serde_json::json!({"file": f, "count": c})).collect::<Vec<_>>(),
                    "total": total,
                });
                crate::output::render_value(&mut writer, &json, format)?;
            }
        }
        _ => {
            if files.len() == 1 {
                writeln!(writer, "{}", results[0].1)?;
            } else {
                for (file, count) in &results {
                    writeln!(writer, "{count}\t{file}")?;
                }
                writeln!(writer, "{total}\ttotal")?;
            }
        }
    }

    Ok(())
}
