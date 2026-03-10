use std::path::Path;

pub fn run(
    file: &str,
    rows_per_file: Option<usize>,
    partition_by: Option<&str>,
    output_dir: &str,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(output_dir)?;

    if let Some(col) = partition_by {
        split_by_partition(file, col, output_dir)
    } else {
        let chunk_size = rows_per_file.unwrap_or(100_000);
        split_by_rows(file, chunk_size, output_dir)
    }
}

fn split_by_rows(file: &str, chunk_size: usize, output_dir: &str) -> anyhow::Result<()> {
    let opts = pq_core::reader::ReadOptions::default();
    let (batches, _schema) = pq_core::reader::open_batches(file, &opts)?;

    // Flatten into one big list of rows across batches
    let total_rows: usize = batches.iter().map(|b| b.num_rows()).sum();
    if total_rows == 0 {
        anyhow::bail!("No rows to split");
    }

    let stem = Path::new(file)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("part");

    let write_opts = pq_core::writer::WriteOptions::default();

    let mut file_idx = 0;
    let mut row_offset = 0;
    let mut batch_idx = 0;
    let mut batch_row = 0;

    while row_offset < total_rows {
        let rows_this_chunk = chunk_size.min(total_rows - row_offset);
        let mut chunk_batches: Vec<arrow::array::RecordBatch> = Vec::new();
        let mut remaining = rows_this_chunk;

        while remaining > 0 && batch_idx < batches.len() {
            let batch = &batches[batch_idx];
            let available = batch.num_rows() - batch_row;
            let take = remaining.min(available);

            let sliced = batch.slice(batch_row, take);
            chunk_batches.push(sliced);

            batch_row += take;
            remaining -= take;

            if batch_row >= batch.num_rows() {
                batch_idx += 1;
                batch_row = 0;
            }
        }

        let out_path = Path::new(output_dir).join(format!("{stem}_{:04}.parquet", file_idx));
        let chunk_rows: usize = chunk_batches.iter().map(|b| b.num_rows()).sum();
        pq_core::writer::write_batches(&out_path, &chunk_batches, &write_opts)?;
        eprintln!("Wrote {} rows to {}", chunk_rows, out_path.display());

        row_offset += rows_this_chunk;
        file_idx += 1;
    }

    eprintln!("Split {total_rows} rows into {file_idx} files in {output_dir}");
    Ok(())
}

fn split_by_partition(file: &str, column: &str, output_dir: &str) -> anyhow::Result<()> {
    let opts = pq_core::reader::ReadOptions::default();
    let (batches, schema) = pq_core::reader::open_batches(file, &opts)?;

    // Find column index
    let col_idx = schema
        .fields()
        .iter()
        .position(|f| f.name() == column)
        .ok_or_else(|| anyhow::anyhow!("Column '{}' not found", column))?;

    use std::collections::HashMap;
    let mut partition_batches: HashMap<String, Vec<arrow::array::RecordBatch>> = HashMap::new();

    for batch in &batches {
        let col = batch.column(col_idx);
        let formatter =
            arrow::util::display::ArrayFormatter::try_new(col.as_ref(), &Default::default())?;

        // Group row indices by partition value
        let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
        for i in 0..batch.num_rows() {
            let key = if col.is_null(i) {
                "__null__".to_string()
            } else {
                formatter.value(i).to_string()
            };
            groups.entry(key).or_default().push(i);
        }

        // Create sub-batches per partition
        for (key, indices) in groups {
            let index_array = arrow::array::UInt32Array::from(
                indices.iter().map(|&i| i as u32).collect::<Vec<_>>(),
            );
            let taken_columns: Vec<arrow::array::ArrayRef> = batch
                .columns()
                .iter()
                .map(|col| arrow::compute::take(col.as_ref(), &index_array, None).unwrap())
                .collect();
            let sub_batch =
                arrow::array::RecordBatch::try_new(batch.schema(), taken_columns)?;
            partition_batches.entry(key).or_default().push(sub_batch);
        }
    }

    let write_opts = pq_core::writer::WriteOptions::default();
    let stem = Path::new(file)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("part");

    let mut total_files = 0;
    for (key, batches) in &partition_batches {
        let safe_key = key.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
        let out_path =
            Path::new(output_dir).join(format!("{column}={safe_key}")).join(format!("{stem}.parquet"));
        std::fs::create_dir_all(out_path.parent().unwrap())?;
        let rows: usize = batches.iter().map(|b| b.num_rows()).sum();
        pq_core::writer::write_batches(&out_path, batches, &write_opts)?;
        eprintln!("Wrote {rows} rows to {}", out_path.display());
        total_files += 1;
    }

    eprintln!(
        "Split into {total_files} partitions by '{column}' in {output_dir}"
    );
    Ok(())
}
