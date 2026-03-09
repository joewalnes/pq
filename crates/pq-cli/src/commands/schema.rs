use std::io::Write;
use std::path::Path;

use pq_core::schema::{read_arrow_schema, schema_to_ddl, schema_to_fields};

use crate::cli::SchemaFormat;
use crate::output::Format;

pub fn run(file: &str, schema_format: &SchemaFormat, output_format: Format) -> anyhow::Result<()> {
    let path = Path::new(file);
    let schema = read_arrow_schema(path)?;

    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    match schema_format {
        SchemaFormat::Tree => match output_format {
            Format::Json | Format::JsonLines => {
                let fields = schema_to_fields(&schema);
                let json = serde_json::to_value(&fields)?;
                crate::output::render_value(&mut writer, &json, output_format)?;
            }
            _ => {
                print_tree(&mut writer, &schema)?;
            }
        },
        SchemaFormat::Json => {
            let fields = schema_to_fields(&schema);
            serde_json::to_writer_pretty(&mut writer, &fields)?;
            writeln!(writer)?;
        }
        SchemaFormat::JsonSchema => {
            let json_schema = arrow_schema_to_json_schema(&schema);
            serde_json::to_writer_pretty(&mut writer, &json_schema)?;
            writeln!(writer)?;
        }
        SchemaFormat::Arrow => {
            writeln!(writer, "{schema:#?}")?;
        }
        SchemaFormat::Ddl => {
            let table_name = Path::new(file)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("data");
            writeln!(writer, "{}", schema_to_ddl(&schema, table_name))?;
        }
    }

    Ok(())
}

fn print_tree(writer: &mut dyn Write, schema: &arrow::datatypes::Schema) -> std::io::Result<()> {
    writeln!(writer, "Schema ({} columns):", schema.fields().len())?;
    for (i, field) in schema.fields().iter().enumerate() {
        let is_last = i == schema.fields().len() - 1;
        let prefix = if is_last { "└── " } else { "├── " };
        let nullable = if field.is_nullable() {
            " (nullable)"
        } else {
            ""
        };
        writeln!(
            writer,
            "{prefix}{}: {}{}",
            field.name(),
            pq_core::schema::schema_to_fields(&arrow::datatypes::Schema::new(vec![field
                .as_ref()
                .clone()]))[0]
                .data_type,
            nullable,
        )?;
    }
    Ok(())
}

fn arrow_schema_to_json_schema(schema: &arrow::datatypes::Schema) -> serde_json::Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();

    for field in schema.fields() {
        let json_type = arrow_type_to_json_schema_type(field.data_type());
        properties.insert(field.name().clone(), json_type);
        if !field.is_nullable() {
            required.push(serde_json::Value::String(field.name().clone()));
        }
    }

    serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "properties": properties,
        "required": required,
    })
}

fn arrow_type_to_json_schema_type(dt: &arrow::datatypes::DataType) -> serde_json::Value {
    use arrow::datatypes::DataType;
    match dt {
        DataType::Boolean => serde_json::json!({"type": "boolean"}),
        DataType::Int8
        | DataType::Int16
        | DataType::Int32
        | DataType::Int64
        | DataType::UInt8
        | DataType::UInt16
        | DataType::UInt32
        | DataType::UInt64 => {
            serde_json::json!({"type": "integer"})
        }
        DataType::Float16 | DataType::Float32 | DataType::Float64 => {
            serde_json::json!({"type": "number"})
        }
        DataType::Utf8 | DataType::LargeUtf8 => serde_json::json!({"type": "string"}),
        DataType::Date32 | DataType::Date64 => {
            serde_json::json!({"type": "string", "format": "date"})
        }
        DataType::Timestamp(_, _) => {
            serde_json::json!({"type": "string", "format": "date-time"})
        }
        DataType::List(inner) | DataType::LargeList(inner) => {
            serde_json::json!({
                "type": "array",
                "items": arrow_type_to_json_schema_type(inner.data_type())
            })
        }
        DataType::Struct(fields) => {
            let mut props = serde_json::Map::new();
            for field in fields {
                props.insert(
                    field.name().clone(),
                    arrow_type_to_json_schema_type(field.data_type()),
                );
            }
            serde_json::json!({"type": "object", "properties": props})
        }
        _ => serde_json::json!({"type": "string"}),
    }
}
