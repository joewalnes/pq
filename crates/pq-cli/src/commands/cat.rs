use std::io::Write;

use pq_core::reader::{open_batches, ReadOptions};

use crate::output::{self, Format};

pub fn run(
    file: &str,
    limit: Option<usize>,
    offset: Option<usize>,
    columns: Option<Vec<String>>,
    where_clause: Option<&str>,
    jq_filter: Option<&str>,
    format: Format,
) -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    // If we have a WHERE clause, use DataFusion SQL
    if let Some(where_clause) = where_clause {
        let rt = tokio::runtime::Runtime::new()?;
        let cols_ref: Option<&[String]> = columns.as_deref();
        let batches = rt.block_on(pq_query::sql::query_with_where(
            file,
            cols_ref,
            where_clause,
            limit,
            offset,
        ))?;

        if let Some(jq_filter) = jq_filter {
            apply_jq_and_output(&mut writer, &batches, jq_filter, format)?;
        } else {
            output::render_batches(&mut writer, &batches, format)?;
        }
        return Ok(());
    }

    let opts = ReadOptions {
        columns,
        limit,
        offset,
        batch_size: 8192,
    };

    let (batches, _schema) = open_batches(file, &opts)?;

    if let Some(jq_filter) = jq_filter {
        apply_jq_and_output(&mut writer, &batches, jq_filter, format)?;
    } else {
        output::render_batches(&mut writer, &batches, format)?;
    }

    Ok(())
}

fn apply_jq_and_output(
    writer: &mut dyn Write,
    batches: &[arrow::array::RecordBatch],
    filter: &str,
    format: Format,
) -> anyhow::Result<()> {
    let json_rows: Vec<serde_json::Value> = batches
        .iter()
        .flat_map(pq_query::convert::batch_to_json_rows)
        .collect();

    let results = pq_query::jq::apply_jq_filter(filter, json_rows, false)?;

    for result in &results {
        crate::output::render_value(writer, result, format)?;
    }
    Ok(())
}
