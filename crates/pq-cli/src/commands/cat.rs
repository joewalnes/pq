use pq_core::reader::{open_batches, ReadOptions};

use crate::output::{self, Format};

// One param per `pq cat` CLI flag, passed straight through from main.rs's
// arg parsing. Bundling these into an options struct would just move the
// same 8 fields to a call-site literal with no gain in clarity, since this
// is the only run() in commands/ that grew past clippy's default threshold.
#[allow(clippy::too_many_arguments)]
pub fn run(
    files: &[String],
    limit: Option<usize>,
    offset: Option<usize>,
    columns: Option<Vec<String>>,
    where_clause: Option<&str>,
    jq_filter: Option<&str>,
    output: Option<&str>,
    format: Format,
) -> anyhow::Result<()> {
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
            let results = apply_jq(&batches, jq_filter)?;
            return write_jq_output(output, &results, format);
        }

        return write_batch_output(output, &batches, format);
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
        let results = apply_jq(&all_batches, jq_filter)?;
        return write_jq_output(output, &results, format);
    }

    write_batch_output(output, &all_batches, format)
}

fn apply_jq(
    batches: &[arrow::array::RecordBatch],
    filter: &str,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let json_rows: Vec<serde_json::Value> = batches
        .iter()
        .flat_map(pq_query::convert::batch_to_json_rows)
        .collect();
    let results = pq_query::jq::apply_jq_filter(filter, json_rows, false)?;
    Ok(results)
}

fn write_batch_output(
    output: Option<&str>,
    batches: &[arrow::array::RecordBatch],
    format: Format,
) -> anyhow::Result<()> {
    match output {
        Some(path) => {
            // Staged inside `write_batches_to_file`: `-O` used to be a bare
            // `File::create` on the user's file, so a write that ran out of
            // space replaced the destination with partial output. The batches
            // are already fully in memory here, which is why `cat X -O X`
            // worked in spite of that; staging makes the *failure* safe too.
            let rows = super::write_output::write_batches_to_file(path, batches)?;
            super::write_output::print_status(path, &format!("Wrote {rows} rows to {path}"));
        }
        None => {
            let stdout = std::io::stdout();
            let mut writer = stdout.lock();
            output::render_batches(&mut writer, batches, format)?;
        }
    }
    Ok(())
}

fn write_jq_output(
    output: Option<&str>,
    results: &[serde_json::Value],
    format: Format,
) -> anyhow::Result<()> {
    match output {
        Some(path) => {
            // Staged inside `json_values_to_file`; see `write_batch_output`.
            // `cat --jq '.' -O <existing file>` on a full disk used to leave
            // the destination emptied or half-written.
            let rows = super::write_output::json_values_to_file(path, results)?;
            super::write_output::print_status(path, &format!("Wrote {rows} rows to {path}"));
        }
        None => {
            let stdout = std::io::stdout();
            let mut writer = stdout.lock();
            for result in results {
                crate::output::render_value(&mut writer, result, format)?;
            }
        }
    }
    Ok(())
}
