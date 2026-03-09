use arrow::array::RecordBatchReader;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;
use std::fs::File;
use std::path::Path;

use pq_core::error::{PqError, Result};

pub struct SliceOptions {
    pub offset: usize,
    pub limit: usize,
    pub output: String,
    pub compression: Compression,
}

pub fn slice_rows(input: &Path, opts: &SliceOptions) -> Result<u64> {
    let file = File::open(input).map_err(|e| PqError::FileOpen {
        path: input.display().to_string(),
        source: e,
    })?;

    let builder = ParquetRecordBatchReaderBuilder::try_new(file)
        .map_err(|e| PqError::ParquetRead {
            path: input.display().to_string(),
            source: e,
        })?
        .with_offset(opts.offset)
        .with_limit(opts.limit);

    let reader = builder.build().map_err(|e| PqError::ParquetRead {
        path: input.display().to_string(),
        source: e,
    })?;

    let schema = reader.schema();
    let out_file = File::create(&opts.output)?;
    let props = WriterProperties::builder()
        .set_compression(opts.compression)
        .build();
    let mut writer = ArrowWriter::try_new(out_file, schema, Some(props))?;

    let mut total_rows = 0u64;
    for batch_result in reader {
        let batch = batch_result?;
        total_rows += batch.num_rows() as u64;
        writer.write(&batch)?;
    }

    writer.close()?;
    Ok(total_rows)
}
