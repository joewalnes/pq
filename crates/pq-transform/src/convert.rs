use arrow::array::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use pq_core::error::{PqError, Result};

use crate::schema_inference;

pub enum InputFormat {
    Json,
    JsonLines,
    Csv,
}

pub struct ConvertOptions {
    pub input_format: InputFormat,
    pub output: String,
    pub compression: Compression,
}

pub fn convert_json_to_parquet(input: &Path, opts: &ConvertOptions) -> Result<u64> {
    if let InputFormat::Csv = opts.input_format {
        return convert_csv_to_parquet(input, opts);
    }

    let content = std::fs::read_to_string(input).map_err(|e| PqError::FileOpen {
        path: input.display().to_string(),
        source: e,
    })?;

    let values: Vec<serde_json::Value> = match opts.input_format {
        InputFormat::Json => {
            let val: serde_json::Value = serde_json::from_str(&content)?;
            match val {
                serde_json::Value::Array(arr) => arr,
                other => vec![other],
            }
        }
        InputFormat::JsonLines => content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(serde_json::from_str)
            .collect::<std::result::Result<Vec<_>, _>>()?,
        InputFormat::Csv => unreachable!(),
    };

    if values.is_empty() {
        return Err(PqError::Other("No data to convert".to_string()));
    }

    let schema = schema_inference::infer_schema_from_json(&values)?;
    let batches = schema_inference::json_values_to_batches(&values, &schema)?;

    // This path already read the whole input into memory above
    // (`read_to_string`), so `-o` naming the input was never destructive here.
    // It is staged anyway so that every writer in the crate behaves the same
    // way, and so a future switch to streaming JSON parsing cannot
    // reintroduce the loss.
    crate::output_guard::with_atomic_output(&opts.output, |out_path| {
        let out_file = File::create(out_path)?;
        let props = WriterProperties::builder()
            .set_compression(opts.compression)
            .build();
        let mut writer = ArrowWriter::try_new(out_file, Arc::new(schema), Some(props))?;

        let mut total_rows = 0u64;
        for batch in &batches {
            total_rows += batch.num_rows() as u64;
            writer.write(batch)?;
        }

        writer.close()?;
        Ok(total_rows)
    })
}

fn convert_csv_to_parquet(input: &Path, opts: &ConvertOptions) -> Result<u64> {
    // Infer schema first
    let file = File::open(input).map_err(|e| PqError::FileOpen {
        path: input.display().to_string(),
        source: e,
    })?;
    let (schema, _) = arrow::csv::reader::Format::default()
        .with_header(true)
        .infer_schema(file, Some(100))?;

    // Now read with inferred schema
    let file = File::open(input).map_err(|e| PqError::FileOpen {
        path: input.display().to_string(),
        source: e,
    })?;
    let csv_reader = arrow::csv::ReaderBuilder::new(Arc::new(schema.clone()))
        .with_header(true)
        .with_batch_size(8192)
        .build(file)?;

    // Staged write. `csv_reader` is lazy: pre-fix, `pq import data.csv -o
    // data.csv` truncated the CSV here, then read zero rows from the now-empty
    // file, and reported "Converted 0 rows" with exit status 0 — a silent,
    // total, plausible-looking data loss.
    crate::output_guard::with_atomic_output(&opts.output, move |out_path| {
        let out_file = File::create(out_path)?;
        let props = WriterProperties::builder()
            .set_compression(opts.compression)
            .build();
        let mut writer = ArrowWriter::try_new(out_file, Arc::new(schema), Some(props))?;

        let mut total_rows = 0u64;
        for batch_result in csv_reader {
            let batch: RecordBatch = batch_result?;
            total_rows += batch.num_rows() as u64;
            writer.write(&batch)?;
        }

        writer.close()?;
        Ok(total_rows)
    })
}
