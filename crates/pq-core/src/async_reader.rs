use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use arrow::array::RecordBatch;
use arrow::datatypes::Schema;
use futures::TryStreamExt;
use object_store::ObjectMeta;
use parquet::arrow::arrow_reader::ArrowReaderMetadata;
use parquet::arrow::async_reader::{ParquetObjectReader, ParquetRecordBatchStreamBuilder};
use parquet::arrow::ProjectionMask;
use parquet::file::metadata::ParquetMetaData;

use crate::error::{PqError, Result};
use crate::reader::ReadOptions;
use crate::source;

// ---------------------------------------------------------------------------
// Metadata cache — avoid repeated HEAD + footer reads for the same URL
// ---------------------------------------------------------------------------

struct CachedMeta {
    object_meta: ObjectMeta,
    arrow_meta: ArrowReaderMetadata,
}

fn metadata_cache() -> &'static Mutex<HashMap<String, CachedMeta>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CachedMeta>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Read record batches from a remote URL using range requests.
pub async fn read_batches(
    url: &str,
    opts: &ReadOptions,
) -> Result<(Vec<RecordBatch>, Arc<Schema>)> {
    let (batches, schema, _) = read_batches_with_row_count(url, opts).await?;
    Ok((batches, schema))
}

/// Read record batches and total row count from a single metadata fetch.
pub async fn read_batches_with_row_count(
    url: &str,
    opts: &ReadOptions,
) -> Result<(Vec<RecordBatch>, Arc<Schema>, i64)> {
    let mut builder = stream_builder(url).await?;
    builder = builder.with_batch_size(opts.batch_size);

    let total_rows = builder.metadata().file_metadata().num_rows();

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

    Ok((batches, schema, total_rows))
}

/// Read parquet metadata and file size from a remote URL.
pub async fn read_metadata(url: &str) -> Result<(ParquetMetaData, u64)> {
    let builder = stream_builder(url).await?;
    let file_size = {
        let guard = metadata_cache().lock().unwrap();
        guard
            .get(url)
            .map(|c| c.object_meta.size as u64)
            .unwrap_or(0)
    };
    Ok((builder.metadata().as_ref().clone(), file_size))
}

/// Read the Arrow schema from a remote URL.
pub async fn read_arrow_schema(url: &str) -> Result<Schema> {
    let builder = stream_builder(url).await?;
    Ok(builder.schema().as_ref().clone())
}

/// Read Arrow schema and total row count from metadata only (no data read).
pub async fn read_schema_and_row_count(url: &str) -> Result<(Arc<Schema>, i64)> {
    let builder = stream_builder(url).await?;
    let total_rows = builder.metadata().file_metadata().num_rows();
    let schema = builder.schema().clone();
    Ok((schema, total_rows))
}

/// Create a ParquetRecordBatchStreamBuilder for a remote URL.
///
/// On the first call for a given URL, this performs a HEAD request (for file size)
/// and reads the parquet footer (metadata). Subsequent calls reuse the cached
/// metadata, eliminating those HTTP round-trips entirely.
async fn stream_builder(url: &str) -> Result<ParquetRecordBatchStreamBuilder<ParquetObjectReader>> {
    let (store, path) = source::parse_url(url)?;

    // Check metadata cache
    {
        let guard = metadata_cache().lock().unwrap();
        if let Some(cached) = guard.get(url) {
            let reader = ParquetObjectReader::new(store, cached.object_meta.clone());
            return Ok(ParquetRecordBatchStreamBuilder::new_with_metadata(
                reader,
                cached.arrow_meta.clone(),
            ));
        }
    }

    // Cache miss: HEAD + footer read
    let object_meta = store
        .head(&path)
        .await
        .map_err(|e| PqError::ObjectStore(e.to_string()))?;
    let reader = ParquetObjectReader::new(store.clone(), object_meta.clone());

    let arrow_meta = ArrowReaderMetadata::load_async(&mut reader.clone(), Default::default())
        .await
        .map_err(|e| PqError::ParquetRead {
            path: url.to_string(),
            source: e,
        })?;

    // Cache for future calls
    metadata_cache().lock().unwrap().insert(
        url.to_string(),
        CachedMeta {
            object_meta: object_meta.clone(),
            arrow_meta: arrow_meta.clone(),
        },
    );

    let reader = ParquetObjectReader::new(store, object_meta);
    Ok(ParquetRecordBatchStreamBuilder::new_with_metadata(
        reader, arrow_meta,
    ))
}
