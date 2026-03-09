use std::io::Write;

use pq_core::reader::{open_batches, ReadOptions};

use crate::output::Format;

pub fn run(
    file: &str,
    filter: &str,
    slurp: bool,
    raw_output: bool,
    format: Format,
) -> anyhow::Result<()> {
    let opts = ReadOptions::default();
    let (batches, _schema) = open_batches(file, &opts)?;

    let json_rows: Vec<serde_json::Value> = batches
        .iter()
        .flat_map(pq_query::convert::batch_to_json_rows)
        .collect();

    let results = pq_query::jq::apply_jq_filter(filter, json_rows, slurp)?;

    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    for result in &results {
        if raw_output {
            if let serde_json::Value::String(s) = result {
                writeln!(writer, "{s}")?;
            } else {
                serde_json::to_writer(&mut writer, result)?;
                writeln!(writer)?;
            }
        } else {
            crate::output::render_value(&mut writer, result, format)?;
        }
    }

    Ok(())
}
