use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;
use std::fs::File;
use std::path::Path;

use pq_core::error::{PqError, Result};

#[derive(Debug, Clone, Copy)]
pub enum SchemaMode {
    Strict,
    Union,
    Intersect,
}

pub struct MergeOptions {
    pub schema_mode: SchemaMode,
    pub output: String,
    pub compression: Compression,
}

pub fn merge_files(inputs: &[&Path], opts: &MergeOptions) -> Result<u64> {
    if inputs.is_empty() {
        return Err(PqError::Other("No input files provided".to_string()));
    }

    // Read schema from first file
    let first_file = File::open(inputs[0]).map_err(|e| PqError::FileOpen {
        path: inputs[0].display().to_string(),
        source: e,
    })?;
    let first_reader =
        ParquetRecordBatchReaderBuilder::try_new(first_file).map_err(|e| PqError::ParquetRead {
            path: inputs[0].display().to_string(),
            source: e,
        })?;
    let output_schema = first_reader.schema().clone();

    let out_file = File::create(&opts.output)?;
    let props = WriterProperties::builder()
        .set_compression(opts.compression)
        .build();
    let mut writer = ArrowWriter::try_new(out_file, output_schema.clone(), Some(props))?;

    let mut total_rows = 0u64;

    for input_path in inputs {
        let file = File::open(input_path).map_err(|e| PqError::FileOpen {
            path: input_path.display().to_string(),
            source: e,
        })?;
        let reader = ParquetRecordBatchReaderBuilder::try_new(file)
            .map_err(|e| PqError::ParquetRead {
                path: input_path.display().to_string(),
                source: e,
            })?
            .build()
            .map_err(|e| PqError::ParquetRead {
                path: input_path.display().to_string(),
                source: e,
            })?;

        for batch_result in reader {
            let batch = batch_result?;
            total_rows += batch.num_rows() as u64;
            writer.write(&batch)?;
        }
    }

    writer.close()?;
    Ok(total_rows)
}
