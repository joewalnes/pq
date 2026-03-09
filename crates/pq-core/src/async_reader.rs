use arrow::array::RecordBatch;
use arrow::datatypes::Schema;
use futures::TryStreamExt;
use parquet::arrow::async_reader::{ParquetObjectReader, ParquetRecordBatchStreamBuilder};
use parquet::arrow::ProjectionMask;
use parquet::file::metadata::ParquetMetaData;
use std::sync::Arc;

use crate::error::{PqError, Result};
use crate::reader::ReadOptions;
use crate::source;

/// Read record batches from a remote URL using range requests.
pub async fn read_batches(
    url: &str,
    opts: &ReadOptions,
) -> Result<(Vec<RecordBatch>, Arc<Schema>)> {
    let mut builder = stream_builder(url).await?;
    builder = builder.with_batch_size(opts.batch_size);

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
        let mask = ProjectionMask::roots(builder.parquet_schema(), indices.iter().copied());
        builder = builder.with_projection(mask);
    }

    if let Some(offset) = opts.offset {
        builder = builder.with_offset(offset);
    }
    if let Some(limit) = opts.limit {
        builder = builder.with_limit(limit);
    }

    let schema = builder.schema().clone();
    let stream = builder.build().map_err(|e| PqError::ParquetRead {
        path: url.to_string(),
        source: e,
    })?;
    let batches: Vec<RecordBatch> = stream.try_collect().await?;

    Ok((batches, schema))
}

/// Read parquet metadata and file size from a remote URL.
pub async fn read_metadata(url: &str) -> Result<(ParquetMetaData, u64)> {
    let (store, path) = source::parse_url(url)?;
    let meta = store
        .head(&path)
        .await
        .map_err(|e| PqError::ObjectStore(e.to_string()))?;
    let file_size = meta.size as u64;
    let reader = ParquetObjectReader::new(store, meta);

    let builder = ParquetRecordBatchStreamBuilder::new(reader)
        .await
        .map_err(|e| PqError::ParquetRead {
            path: url.to_string(),
            source: e,
        })?;

    Ok((builder.metadata().as_ref().clone(), file_size))
}

/// Read the Arrow schema from a remote URL.
pub async fn read_arrow_schema(url: &str) -> Result<Schema> {
    let builder = stream_builder(url).await?;
    Ok(builder.schema().as_ref().clone())
}

/// Create a ParquetRecordBatchStreamBuilder for a remote URL.
async fn stream_builder(url: &str) -> Result<ParquetRecordBatchStreamBuilder<ParquetObjectReader>> {
    let (store, path) = source::parse_url(url)?;
    let meta = store
        .head(&path)
        .await
        .map_err(|e| PqError::ObjectStore(e.to_string()))?;
    let reader = ParquetObjectReader::new(store, meta);

    ParquetRecordBatchStreamBuilder::new(reader)
        .await
        .map_err(|e| PqError::ParquetRead {
            path: url.to_string(),
            source: e,
        })
}
