use arrow::array::{RecordBatch, RecordBatchReader};
use arrow::datatypes::Schema;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use crate::error::{PqError, Result};

pub struct ReadOptions {
    pub columns: Option<Vec<String>>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub batch_size: usize,
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            columns: None,
            limit: None,
            offset: None,
            batch_size: 8192,
        }
    }
}

pub fn read_batches(path: &Path, opts: &ReadOptions) -> Result<(Vec<RecordBatch>, Arc<Schema>)> {
    let file = File::open(path).map_err(|e| PqError::FileOpen {
        path: path.display().to_string(),
        source: e,
    })?;

    let mut builder = ParquetRecordBatchReaderBuilder::try_new(file)
        .map_err(|e| PqError::ParquetRead {
            path: path.display().to_string(),
            source: e,
        })?
        .with_batch_size(opts.batch_size);

    // Apply column projection
    if let Some(ref columns) = opts.columns {
        let schema = builder.schema().clone();
        let mut indices = Vec::new();
        for col_name in columns {
            let idx = schema
                .fields()
                .iter()
                .position(|f| f.name() == col_name)
                .ok_or_else(|| PqError::ColumnNotFound {
                    name: col_name.clone(),
                })?;
            indices.push(idx);
        }
        let mask = parquet::arrow::ProjectionMask::leaves(
            builder.parquet_schema(),
            indices.iter().copied(),
        );
        builder = builder.with_projection(mask);
    }

    // Apply offset and limit
    if let Some(offset) = opts.offset {
        builder = builder.with_offset(offset);
    }
    if let Some(limit) = opts.limit {
        builder = builder.with_limit(limit);
    }

    let reader = builder.build().map_err(|e| PqError::ParquetRead {
        path: path.display().to_string(),
        source: e,
    })?;

    let schema = reader.schema();
    let mut batches = Vec::new();
    for batch_result in reader {
        let batch = batch_result?;
        batches.push(batch);
    }

    Ok((batches, schema))
}

/// Read rows from the end of the file
pub fn read_tail(
    path: &Path,
    n: usize,
    columns: Option<Vec<String>>,
) -> Result<(Vec<RecordBatch>, Arc<Schema>)> {
    let metadata = crate::metadata::read_metadata(path)?;
    let total_rows = metadata.file_metadata().num_rows() as usize;

    let offset = total_rows.saturating_sub(n);
    let limit = if total_rows > n { n } else { total_rows };

    let opts = ReadOptions {
        columns,
        limit: Some(limit),
        offset: Some(offset),
        batch_size: 8192,
    };

    read_batches(path, &opts)
}

/// Get row count from metadata without reading data
pub fn row_count(path: &Path) -> Result<i64> {
    let metadata = crate::metadata::read_metadata(path)?;
    Ok(metadata.file_metadata().num_rows())
}
