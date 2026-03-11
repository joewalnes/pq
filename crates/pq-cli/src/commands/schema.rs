use std::io::Write;
use std::path::Path;

use pq_core::schema::{open_arrow_schema, schema_to_ddl, schema_to_fields, schema_to_pyarrow};

use crate::cli::SchemaFormat;
use crate::output::Format;

pub fn run(file: &str, schema_format: &SchemaFormat, output_format: Format) -> anyhow::Result<()> {
    let schema = open_arrow_schema(file)?;

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
        SchemaFormat::Pyarrow => {
            writeln!(writer, "{}", schema_to_pyarrow(&schema))?;
        }
    }

    Ok(())
}

fn print_tree(writer: &mut dyn Write, schema: &arrow::datatypes::Schema) -> std::io::Result<()> {
    writeln!(writer, "Schema ({} columns):", schema.fields().len())?;
    for (i, field) in schema.fields().iter().enumerate() {
        let is_last = i == schema.fields().len() - 1;
        print_field_tree(writer, field, "", is_last)?;
    }
    Ok(())
}

fn print_field_tree(
    writer: &mut dyn Write,
    field: &arrow::datatypes::Field,
    prefix: &str,
    is_last: bool,
) -> std::io::Result<()> {
    use arrow::datatypes::DataType;

    let connector = if is_last { "└── " } else { "├── " };
    let child_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });
    let nullable = if field.is_nullable() {
        " (nullable)"
    } else {
        ""
    };

    match field.data_type() {
        DataType::Struct(fields) => {
            writeln!(
                writer,
                "{prefix}{connector}{}: struct{nullable}",
                field.name()
            )?;
            for (i, child) in fields.iter().enumerate() {
                let child_is_last = i == fields.len() - 1;
                print_field_tree(writer, child, &child_prefix, child_is_last)?;
            }
        }
        DataType::List(inner) | DataType::LargeList(inner) => {
            let type_label = pq_core::schema::format_data_type_public(field.data_type());
            writeln!(
                writer,
                "{prefix}{connector}{}: {type_label}{nullable}",
                field.name()
            )?;
            // If the inner type is a struct, show its children
            if let DataType::Struct(fields) = inner.data_type() {
                for (i, child) in fields.iter().enumerate() {
                    let child_is_last = i == fields.len() - 1;
                    print_field_tree(writer, child, &child_prefix, child_is_last)?;
                }
            }
        }
        DataType::Map(entry_field, _) => {
            writeln!(writer, "{prefix}{connector}{}: map{nullable}", field.name())?;
            if let DataType::Struct(fields) = entry_field.data_type() {
                for (i, child) in fields.iter().enumerate() {
                    let child_is_last = i == fields.len() - 1;
                    print_field_tree(writer, child, &child_prefix, child_is_last)?;
                }
            }
        }
        dt => {
            let type_label = pq_core::schema::format_data_type_public(dt);
            writeln!(
                writer,
                "{prefix}{connector}{}: {type_label}{nullable}",
                field.name()
            )?;
        }
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
        DataType::Decimal128(_, _) | DataType::Decimal256(_, _) => {
            serde_json::json!({"type": "number"})
        }
        DataType::Utf8 | DataType::LargeUtf8 => serde_json::json!({"type": "string"}),
        DataType::Binary | DataType::LargeBinary | DataType::FixedSizeBinary(_) => {
            serde_json::json!({"type": "string", "contentEncoding": "hex"})
        }
        DataType::Date32 | DataType::Date64 => {
            serde_json::json!({"type": "string", "format": "date"})
        }
        DataType::Timestamp(_, _) => {
            serde_json::json!({"type": "string", "format": "date-time"})
        }
        DataType::List(inner) | DataType::LargeList(inner) | DataType::FixedSizeList(inner, _) => {
            serde_json::json!({
                "type": "array",
                "items": arrow_type_to_json_schema_type(inner.data_type())
            })
        }
        DataType::Struct(fields) => {
            let mut props = serde_json::Map::new();
            let mut req = Vec::new();
            for field in fields {
                props.insert(
                    field.name().clone(),
                    arrow_type_to_json_schema_type(field.data_type()),
                );
                if !field.is_nullable() {
                    req.push(serde_json::Value::String(field.name().clone()));
                }
            }
            let mut obj = serde_json::json!({"type": "object", "properties": props});
            if !req.is_empty() {
                obj["required"] = serde_json::Value::Array(req);
            }
            obj
        }
        DataType::Map(entry_field, _) => {
            // Map keys -> additionalProperties
            if let DataType::Struct(fields) = entry_field.data_type() {
                if fields.len() == 2 {
                    let value_type = arrow_type_to_json_schema_type(fields[1].data_type());
                    return serde_json::json!({
                        "type": "object",
                        "additionalProperties": value_type
                    });
                }
            }
            serde_json::json!({"type": "object"})
        }
        _ => serde_json::json!({"type": "string"}),
    }
}
