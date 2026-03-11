use std::path::Path;

pub fn run(
    file: &str,
    rows_per_file: Option<usize>,
    partition_by: Option<&[String]>,
    output_dir: &str,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(output_dir)?;

    match (partition_by, rows_per_file) {
        (Some(cols), Some(chunk_size)) if !cols.is_empty() => {
            // Both: partition first, then chunk each partition by rows
            split_by_partition_and_rows(file, cols, chunk_size, output_dir)
        }
        (Some(cols), _) if !cols.is_empty() => split_by_partition(file, cols, output_dir),
        _ => {
            let chunk_size = rows_per_file.unwrap_or(100_000);
            split_by_rows(file, chunk_size, output_dir)
        }
    }
}

fn split_by_rows(file: &str, chunk_size: usize, output_dir: &str) -> anyhow::Result<()> {
    let opts = pq_core::reader::ReadOptions::default();
    let (batches, _schema) = pq_core::reader::open_batches(file, &opts)?;

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

fn split_by_partition(file: &str, columns: &[String], output_dir: &str) -> anyhow::Result<()> {
    let opts = pq_core::reader::ReadOptions::default();
    let (batches, schema) = pq_core::reader::open_batches(file, &opts)?;

    // Resolve column indices, supporting dot notation for nested fields
    let col_indices: Vec<usize> = columns
        .iter()
        .map(|col| resolve_column_index(&schema, col))
        .collect::<anyhow::Result<Vec<_>>>()?;

    use std::collections::HashMap;
    let mut partition_batches: HashMap<String, Vec<arrow::array::RecordBatch>> = HashMap::new();

    for batch in &batches {
        // Build formatters for each partition column
        let formatters: Vec<arrow::util::display::ArrayFormatter> = col_indices
            .iter()
            .map(|&idx| {
                let col = batch.column(idx);
                arrow::util::display::ArrayFormatter::try_new(col.as_ref(), &Default::default())
            })
            .collect::<Result<Vec<_>, _>>()?;

        // Group row indices by composite partition key
        let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
        for i in 0..batch.num_rows() {
            let key_parts: Vec<String> = col_indices
                .iter()
                .zip(formatters.iter())
                .map(|(&col_idx, fmt)| {
                    if batch.column(col_idx).is_null(i) {
                        "__null__".to_string()
                    } else {
                        fmt.value(i).to_string()
                    }
                })
                .collect();
            // Build Hive-style key: col1=val1/col2=val2
            let key = columns
                .iter()
                .zip(key_parts.iter())
                .map(|(col, val)| format!("{}={}", col_name_leaf(col), safe_path(val)))
                .collect::<Vec<_>>()
                .join("/");
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
            let sub_batch = arrow::array::RecordBatch::try_new(batch.schema(), taken_columns)?;
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
        let out_path = Path::new(output_dir)
            .join(key)
            .join(format!("{stem}.parquet"));
        std::fs::create_dir_all(out_path.parent().unwrap())?;
        let rows: usize = batches.iter().map(|b| b.num_rows()).sum();
        pq_core::writer::write_batches(&out_path, batches, &write_opts)?;
        eprintln!("Wrote {rows} rows to {}", out_path.display());
        total_files += 1;
    }

    let col_names = columns.join(", ");
    eprintln!("Split into {total_files} partitions by '{col_names}' in {output_dir}");
    Ok(())
}

fn split_by_partition_and_rows(
    file: &str,
    columns: &[String],
    chunk_size: usize,
    output_dir: &str,
) -> anyhow::Result<()> {
    let opts = pq_core::reader::ReadOptions::default();
    let (batches, schema) = pq_core::reader::open_batches(file, &opts)?;

    let col_indices: Vec<usize> = columns
        .iter()
        .map(|col| resolve_column_index(&schema, col))
        .collect::<anyhow::Result<Vec<_>>>()?;

    use std::collections::HashMap;
    let mut partition_batches: HashMap<String, Vec<arrow::array::RecordBatch>> = HashMap::new();

    for batch in &batches {
        let formatters: Vec<arrow::util::display::ArrayFormatter> = col_indices
            .iter()
            .map(|&idx| {
                let col = batch.column(idx);
                arrow::util::display::ArrayFormatter::try_new(col.as_ref(), &Default::default())
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
        for i in 0..batch.num_rows() {
            let key_parts: Vec<String> = col_indices
                .iter()
                .zip(formatters.iter())
                .map(|(&col_idx, fmt)| {
                    if batch.column(col_idx).is_null(i) {
                        "__null__".to_string()
                    } else {
                        fmt.value(i).to_string()
                    }
                })
                .collect();
            let key = columns
                .iter()
                .zip(key_parts.iter())
                .map(|(col, val)| format!("{}={}", col_name_leaf(col), safe_path(val)))
                .collect::<Vec<_>>()
                .join("/");
            groups.entry(key).or_default().push(i);
        }

        for (key, indices) in groups {
            let index_array = arrow::array::UInt32Array::from(
                indices.iter().map(|&i| i as u32).collect::<Vec<_>>(),
            );
            let taken_columns: Vec<arrow::array::ArrayRef> = batch
                .columns()
                .iter()
                .map(|col| arrow::compute::take(col.as_ref(), &index_array, None).unwrap())
                .collect();
            let sub_batch = arrow::array::RecordBatch::try_new(batch.schema(), taken_columns)?;
            partition_batches.entry(key).or_default().push(sub_batch);
        }
    }

    // Now chunk each partition by rows
    let write_opts = pq_core::writer::WriteOptions::default();
    let stem = Path::new(file)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("part");

    let mut total_files = 0;
    for (key, part_batches) in &partition_batches {
        let total_rows: usize = part_batches.iter().map(|b| b.num_rows()).sum();
        let mut file_idx = 0;
        let mut row_offset = 0;
        let mut batch_idx = 0;
        let mut batch_row = 0;

        while row_offset < total_rows {
            let rows_this_chunk = chunk_size.min(total_rows - row_offset);
            let mut chunk_batches: Vec<arrow::array::RecordBatch> = Vec::new();
            let mut remaining = rows_this_chunk;

            while remaining > 0 && batch_idx < part_batches.len() {
                let batch = &part_batches[batch_idx];
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

            let out_path = Path::new(output_dir)
                .join(key)
                .join(format!("{stem}_{:04}.parquet", file_idx));
            std::fs::create_dir_all(out_path.parent().unwrap())?;
            let chunk_rows: usize = chunk_batches.iter().map(|b| b.num_rows()).sum();
            pq_core::writer::write_batches(&out_path, &chunk_batches, &write_opts)?;
            eprintln!("Wrote {chunk_rows} rows to {}", out_path.display());

            row_offset += rows_this_chunk;
            file_idx += 1;
            total_files += 1;
        }
    }

    let col_names = columns.join(", ");
    eprintln!(
        "Split into {total_files} files by '{col_names}' (chunked by {chunk_size} rows) in {output_dir}"
    );
    Ok(())
}

/// Resolve a column name (supporting dot notation) to its index in the schema.
fn resolve_column_index(schema: &arrow::datatypes::Schema, col: &str) -> anyhow::Result<usize> {
    // First try exact match
    if let Some(idx) = schema.fields().iter().position(|f| f.name() == col) {
        return Ok(idx);
    }

    // Try dot notation: only resolve the top-level field name
    if col.contains('.') {
        let top_level = col.split('.').next().unwrap();
        if let Some(idx) = schema.fields().iter().position(|f| f.name() == top_level) {
            return Ok(idx);
        }
    }

    anyhow::bail!("Column '{}' not found in schema", col)
}

/// Get the leaf name from a dot-notation column path.
fn col_name_leaf(col: &str) -> &str {
    col.rsplit('.').next().unwrap_or(col)
}

/// Sanitize a value for use in a filesystem path.
fn safe_path(val: &str) -> String {
    val.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_")
}
