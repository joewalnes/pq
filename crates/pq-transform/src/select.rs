use arrow::array::RecordBatchReader;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;
use std::fs::File;
use std::path::Path;

use pq_core::error::{PqError, Result};

pub struct SelectOptions {
    pub columns: Vec<String>,
    pub output: String,
    pub compression: Compression,
}

pub fn select_columns(input: &Path, opts: &SelectOptions) -> Result<u64> {
    let file = File::open(input).map_err(|e| PqError::FileOpen {
        path: input.display().to_string(),
        source: e,
    })?;

    let builder =
        ParquetRecordBatchReaderBuilder::try_new(file).map_err(|e| PqError::ParquetRead {
            path: input.display().to_string(),
            source: e,
        })?;

    let schema = builder.schema().clone();

    // Find column indices
    let mut indices = Vec::new();
    for col_name in &opts.columns {
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
    let mask =
        parquet::arrow::ProjectionMask::roots(builder.parquet_schema(), indices.iter().copied());
    let reader = builder
        .with_projection(mask)
        .build()
        .map_err(|e| PqError::ParquetRead {
            path: input.display().to_string(),
            source: e,
        })?;

    let out_schema = reader.schema();
    let out_file = File::create(&opts.output)?;
    let props = WriterProperties::builder()
        .set_compression(opts.compression)
        .build();
    let mut writer = ArrowWriter::try_new(out_file, out_schema, Some(props))?;

    let mut total_rows = 0u64;
    for batch_result in reader {
        let batch = batch_result?;
        total_rows += batch.num_rows() as u64;
        writer.write(&batch)?;
    }

    writer.close()?;
    Ok(total_rows)
}
