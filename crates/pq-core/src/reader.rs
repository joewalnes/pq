use arrow::array::{RecordBatch, RecordBatchReader};
use arrow::datatypes::Schema;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use crate::error::{PqError, Result};
use crate::source;

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
    let (batches, schema, _) = read_batches_with_row_count(path, opts)?;
    Ok((batches, schema))
}

pub fn read_batches_with_row_count(
    path: &Path,
    opts: &ReadOptions,
) -> Result<(Vec<RecordBatch>, Arc<Schema>, i64)> {
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

    let total_rows = builder.metadata().file_metadata().num_rows();

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
        // Use roots() not leaves() — top-level field indices differ from leaf
        // column indices when the schema contains structs, lists, or maps.
        let mask = parquet::arrow::ProjectionMask::roots(
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

    Ok((batches, schema, total_rows))
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

/// Read schema and row count from metadata without reading data.
pub fn read_schema_and_row_count(path: &Path) -> Result<(Arc<Schema>, i64)> {
    let file = File::open(path).map_err(|e| PqError::FileOpen {
        path: path.display().to_string(),
        source: e,
    })?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file).map_err(|e| PqError::ParquetRead {
        path: path.display().to_string(),
        source: e,
    })?;
    let total_rows = builder.metadata().file_metadata().num_rows();
    let schema = builder.schema().clone();
    Ok((schema, total_rows))
}

// ---------------------------------------------------------------------------
// Universal functions: accept a path or URL string, dispatch accordingly
// ---------------------------------------------------------------------------

/// Read batches from a local path or remote URL.
pub fn open_batches(location: &str, opts: &ReadOptions) -> Result<(Vec<RecordBatch>, Arc<Schema>)> {
    if source::is_url(location) {
        source::block_on_async(crate::async_reader::read_batches(location, opts))
    } else {
        read_batches(Path::new(location), opts)
    }
}

/// Read batches and total row count from a single metadata read.
pub fn open_batches_with_row_count(
    location: &str,
    opts: &ReadOptions,
) -> Result<(Vec<RecordBatch>, Arc<Schema>, i64)> {
    if source::is_url(location) {
        source::block_on_async(crate::async_reader::read_batches_with_row_count(
            location, opts,
        ))
    } else {
        read_batches_with_row_count(Path::new(location), opts)
    }
}

/// Read tail rows from a local path or remote URL.
pub fn open_tail(
    location: &str,
    n: usize,
    columns: Option<Vec<String>>,
) -> Result<(Vec<RecordBatch>, Arc<Schema>)> {
    if source::is_url(location) {
        let (meta, _size) = source::block_on_async(crate::async_reader::read_metadata(location))?;
        let total_rows = meta.file_metadata().num_rows() as usize;
        let offset = total_rows.saturating_sub(n);
        let limit = if total_rows > n { n } else { total_rows };
        let opts = ReadOptions {
            columns,
            limit: Some(limit),
            offset: Some(offset),
            batch_size: 8192,
        };
        source::block_on_async(crate::async_reader::read_batches(location, &opts))
    } else {
        read_tail(Path::new(location), n, columns)
    }
}

/// Read schema and total row count from metadata only (no data read).
pub fn open_metadata(location: &str) -> Result<(Arc<Schema>, i64)> {
    if source::is_url(location) {
        source::block_on_async(crate::async_reader::read_schema_and_row_count(location))
    } else {
        read_schema_and_row_count(Path::new(location))
    }
}

/// Get row count from a local path or remote URL.
pub fn open_row_count(location: &str) -> Result<i64> {
    if source::is_url(location) {
        let (meta, _size) = source::block_on_async(crate::async_reader::read_metadata(location))?;
        Ok(meta.file_metadata().num_rows())
    } else {
        row_count(Path::new(location))
    }
}
