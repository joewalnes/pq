use std::collections::HashSet;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use arrow::array::{new_null_array, RecordBatch};
use arrow::datatypes::{Field, Schema, SchemaRef};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;

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

/// Read the schema from a parquet file without reading any data.
fn read_schema(path: &Path) -> Result<SchemaRef> {
    let file = File::open(path).map_err(|e| PqError::FileOpen {
        path: path.display().to_string(),
        source: e,
    })?;
    let reader =
        ParquetRecordBatchReaderBuilder::try_new(file).map_err(|e| PqError::ParquetRead {
            path: path.display().to_string(),
            source: e,
        })?;
    Ok(reader.schema().clone())
}

/// Compute the output schema from all input schemas according to the schema mode.
fn resolve_schema(schemas: &[SchemaRef], mode: SchemaMode) -> Result<SchemaRef> {
    match mode {
        SchemaMode::Strict => {
            let first = &schemas[0];
            for (i, schema) in schemas.iter().enumerate().skip(1) {
                if schema.as_ref() != first.as_ref() {
                    return Err(PqError::Other(format!(
                        "Schema mismatch in file {i}: expected {} fields matching first file, \
                         got {} fields. Use --schema-mode union or intersect to reconcile.",
                        first.fields().len(),
                        schema.fields().len(),
                    )));
                }
            }
            Ok(first.clone())
        }
        SchemaMode::Union => {
            // Preserve field order: first file's fields first, then new fields
            // from subsequent files in the order they appear.
            let mut seen = HashSet::new();
            let mut union_fields: Vec<Arc<Field>> = Vec::new();
            for schema in schemas {
                for field in schema.fields() {
                    if seen.insert(field.name().clone()) {
                        // Union columns may be missing in some files, so mark nullable
                        let f = if field.is_nullable() {
                            field.clone()
                        } else {
                            Arc::new(field.as_ref().clone().with_nullable(true))
                        };
                        union_fields.push(f);
                    }
                }
            }
            Ok(Arc::new(Schema::new(union_fields)))
        }
        SchemaMode::Intersect => {
            // Keep only fields that appear in ALL schemas, in first file's order.
            let first = &schemas[0];
            let intersect_fields: Vec<Arc<Field>> = first
                .fields()
                .iter()
                .filter(|field| {
                    schemas
                        .iter()
                        .skip(1)
                        .all(|s| s.field_with_name(field.name()).is_ok())
                })
                .cloned()
                .collect();
            if intersect_fields.is_empty() {
                return Err(PqError::Other(
                    "No columns in common across all input files".to_string(),
                ));
            }
            Ok(Arc::new(Schema::new(intersect_fields)))
        }
    }
}

/// Adapt a batch to match the output schema by reordering, projecting, or
/// adding null columns as needed.
fn adapt_batch(batch: &RecordBatch, output_schema: &SchemaRef) -> Result<RecordBatch> {
    let num_rows = batch.num_rows();
    let columns: Vec<_> = output_schema
        .fields()
        .iter()
        .map(|field| {
            match batch.schema().index_of(field.name()) {
                Ok(idx) => batch.column(idx).clone(),
                Err(_) => {
                    // Column missing in this file — fill with nulls
                    new_null_array(field.data_type(), num_rows)
                }
            }
        })
        .collect();
    RecordBatch::try_new(output_schema.clone(), columns)
        .map_err(|e| PqError::Other(format!("Failed to adapt batch: {e}")))
}

pub fn merge_files(inputs: &[&Path], opts: &MergeOptions) -> Result<u64> {
    if inputs.is_empty() {
        return Err(PqError::Other("No input files provided".to_string()));
    }

    // Read all schemas
    let schemas: Vec<SchemaRef> = inputs
        .iter()
        .map(|p| read_schema(p))
        .collect::<Result<_>>()?;

    let output_schema = resolve_schema(&schemas, opts.schema_mode)?;

    let out_file = File::create(&opts.output)?;
    let props = WriterProperties::builder()
        .set_compression(opts.compression)
        .build();
    let mut writer = ArrowWriter::try_new(out_file, output_schema.clone(), Some(props))?;

    let mut total_rows = 0u64;
    let needs_adapt = !matches!(opts.schema_mode, SchemaMode::Strict);

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
            if needs_adapt {
                let adapted = adapt_batch(&batch, &output_schema)?;
                writer.write(&adapted)?;
            } else {
                writer.write(&batch)?;
            }
        }
    }

    writer.close()?;
    Ok(total_rows)
}
