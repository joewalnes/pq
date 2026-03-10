use std::io::Write;

use pq_core::reader::{open_batches, ReadOptions};

use crate::output::{self, Format};

pub fn run(
    files: &[String],
    limit: Option<usize>,
    offset: Option<usize>,
    columns: Option<Vec<String>>,
    where_clause: Option<&str>,
    jq_filter: Option<&str>,
    format: Format,
) -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    // If we have a WHERE clause, use DataFusion SQL (single file only)
    if let Some(where_clause) = where_clause {
        let file = &files[0];
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

    let mut all_batches = Vec::new();
    let mut remaining_limit = limit;

    for (i, file) in files.iter().enumerate() {
        let opts = ReadOptions {
            columns: columns.clone(),
            limit: remaining_limit,
            offset: if i == 0 { offset } else { None },
            batch_size: 8192,
        };

        let (batches, _schema) = open_batches(file, &opts)?;
        let rows_read: usize = batches.iter().map(|b| b.num_rows()).sum();
        all_batches.extend(batches);

        if let Some(ref mut remaining) = remaining_limit {
            *remaining = remaining.saturating_sub(rows_read);
            if *remaining == 0 {
                break;
            }
        }
    }

    if let Some(jq_filter) = jq_filter {
        apply_jq_and_output(&mut writer, &all_batches, jq_filter, format)?;
    } else {
        output::render_batches(&mut writer, &all_batches, format)?;
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
