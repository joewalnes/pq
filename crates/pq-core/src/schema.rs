use arrow::datatypes::{DataType, Field, Schema};
use parquet::file::reader::{FileReader, SerializedFileReader};
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;

use crate::error::{PqError, Result};
use crate::source;

#[derive(Debug, Clone, Serialize)]
pub struct SchemaField {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub children: Vec<SchemaField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::HashMap<String, String>>,
}

pub fn read_arrow_schema(path: &Path) -> Result<Schema> {
    let file = std::fs::File::open(path).map_err(|e| PqError::FileOpen {
        path: path.display().to_string(),
        source: e,
    })?;
    let reader = SerializedFileReader::new(file).map_err(|e| PqError::ParquetRead {
        path: path.display().to_string(),
        source: e,
    })?;
    let arrow_schema = parquet::arrow::parquet_to_arrow_schema(
        reader.metadata().file_metadata().schema_descr(),
        reader.metadata().file_metadata().key_value_metadata(),
    )?;
    Ok(arrow_schema)
}

pub fn schema_to_fields(schema: &Schema) -> Vec<SchemaField> {
    schema.fields().iter().map(field_to_schema_field).collect()
}

fn field_to_schema_field(field: &Arc<Field>) -> SchemaField {
    let children = match field.data_type() {
        DataType::Struct(fields) => fields.iter().map(field_to_schema_field).collect(),
        DataType::List(inner) | DataType::LargeList(inner) => {
            vec![field_to_schema_field(inner)]
        }
        DataType::Map(inner, _) => {
            if let DataType::Struct(fields) = inner.data_type() {
                fields.iter().map(field_to_schema_field).collect()
            } else {
                vec![field_to_schema_field(inner)]
            }
        }
        _ => Vec::new(),
    };

    let metadata = if field.metadata().is_empty() {
        None
    } else {
        Some(field.metadata().clone())
    };

    SchemaField {
        name: field.name().clone(),
        data_type: format_data_type(field.data_type()),
        nullable: field.is_nullable(),
        children,
        metadata,
    }
}

/// Format a DataType as a human-readable string.
pub fn format_data_type_public(dt: &DataType) -> String {
    format_data_type(dt)
}

fn format_data_type(dt: &DataType) -> String {
    match dt {
        DataType::Null => "null".to_string(),
        DataType::Boolean => "boolean".to_string(),
        DataType::Int8 => "int8".to_string(),
        DataType::Int16 => "int16".to_string(),
        DataType::Int32 => "int32".to_string(),
        DataType::Int64 => "int64".to_string(),
        DataType::UInt8 => "uint8".to_string(),
        DataType::UInt16 => "uint16".to_string(),
        DataType::UInt32 => "uint32".to_string(),
        DataType::UInt64 => "uint64".to_string(),
        DataType::Float16 => "float16".to_string(),
        DataType::Float32 => "float32".to_string(),
        DataType::Float64 => "float64".to_string(),
        DataType::Utf8 | DataType::LargeUtf8 => "string".to_string(),
        DataType::Binary | DataType::LargeBinary => "binary".to_string(),
        DataType::FixedSizeBinary(n) => format!("fixed_binary({n})"),
        DataType::Date32 => "date".to_string(),
        DataType::Date64 => "date".to_string(),
        DataType::Timestamp(unit, tz) => {
            let u = match unit {
                arrow::datatypes::TimeUnit::Second => "s",
                arrow::datatypes::TimeUnit::Millisecond => "ms",
                arrow::datatypes::TimeUnit::Microsecond => "us",
                arrow::datatypes::TimeUnit::Nanosecond => "ns",
            };
            match tz {
                Some(tz) => format!("timestamp({u}, {tz})"),
                None => format!("timestamp({u})"),
            }
        }
        DataType::Time32(_) | DataType::Time64(_) => "time".to_string(),
        DataType::Duration(_) => "duration".to_string(),
        DataType::Decimal128(p, s) | DataType::Decimal256(p, s) => format!("decimal({p},{s})"),
        DataType::List(f) | DataType::LargeList(f) => {
            format!("list<{}>", format_data_type(f.data_type()))
        }
        DataType::FixedSizeList(f, n) => {
            format!("fixed_list<{}, {n}>", format_data_type(f.data_type()))
        }
        DataType::Struct(_) => "struct".to_string(),
        DataType::Map(_, _) => "map".to_string(),
        DataType::Dictionary(k, v) => {
            format!(
                "dictionary<{}, {}>",
                format_data_type(k),
                format_data_type(v)
            )
        }
        _ => format!("{dt:?}"),
    }
}

pub fn schema_to_ddl(schema: &Schema, table_name: &str) -> String {
    let mut ddl = format!("CREATE TABLE {table_name} (\n");
    let fields: Vec<String> = schema
        .fields()
        .iter()
        .map(|f| {
            let sql_type = arrow_type_to_sql(f.data_type());
            let nullable = if f.is_nullable() { "" } else { " NOT NULL" };
            format!("  {} {}{}", f.name(), sql_type, nullable)
        })
        .collect();
    ddl.push_str(&fields.join(",\n"));
    ddl.push_str("\n);");
    ddl
}

fn arrow_type_to_sql(dt: &DataType) -> String {
    match dt {
        DataType::Boolean => "BOOLEAN".to_string(),
        DataType::Int8 | DataType::Int16 => "SMALLINT".to_string(),
        DataType::Int32 => "INTEGER".to_string(),
        DataType::Int64 => "BIGINT".to_string(),
        DataType::UInt8 | DataType::UInt16 => "SMALLINT".to_string(),
        DataType::UInt32 => "INTEGER".to_string(),
        DataType::UInt64 => "BIGINT".to_string(),
        DataType::Float32 => "REAL".to_string(),
        DataType::Float64 => "DOUBLE PRECISION".to_string(),
        DataType::Utf8 | DataType::LargeUtf8 => "TEXT".to_string(),
        DataType::Binary | DataType::LargeBinary | DataType::FixedSizeBinary(_) => {
            "BYTEA".to_string()
        }
        DataType::Date32 | DataType::Date64 => "DATE".to_string(),
        DataType::Timestamp(_, _) => "TIMESTAMP".to_string(),
        DataType::Decimal128(p, s) | DataType::Decimal256(p, s) => format!("DECIMAL({p},{s})"),
        DataType::List(inner) | DataType::LargeList(inner) | DataType::FixedSizeList(inner, _) => {
            format!("{}[]", arrow_type_to_sql(inner.data_type()))
        }
        DataType::Struct(fields) => {
            let field_defs: Vec<String> = fields
                .iter()
                .map(|f| format!("{} {}", f.name(), arrow_type_to_sql(f.data_type())))
                .collect();
            format!("STRUCT({})", field_defs.join(", "))
        }
        DataType::Map(entry, _) => {
            if let DataType::Struct(fields) = entry.data_type() {
                if fields.len() == 2 {
                    return format!(
                        "MAP({}, {})",
                        arrow_type_to_sql(fields[0].data_type()),
                        arrow_type_to_sql(fields[1].data_type()),
                    );
                }
            }
            "MAP(TEXT, TEXT)".to_string()
        }
        _ => "TEXT".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Universal function: accept a path or URL string, dispatch accordingly
// ---------------------------------------------------------------------------

/// Read the Arrow schema from a local path or remote URL.
pub fn open_arrow_schema(location: &str) -> Result<Schema> {
    if source::is_url(location) {
        source::block_on_async(crate::async_reader::read_arrow_schema(location))
    } else {
        read_arrow_schema(Path::new(location))
    }
}
