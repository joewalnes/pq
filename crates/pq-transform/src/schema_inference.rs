use arrow::array::*;
use arrow::datatypes::*;
use serde_json::Value;
use std::sync::Arc;

use pq_core::error::{PqError, Result};

pub fn infer_schema_from_json(values: &[Value]) -> Result<Schema> {
    if values.is_empty() {
        return Err(PqError::Other(
            "Cannot infer schema from empty data".to_string(),
        ));
    }

    let mut fields: Vec<Field> = Vec::new();

    // Collect all keys from all objects
    for value in values {
        if let Value::Object(obj) = value {
            for (key, val) in obj {
                if !fields.iter().any(|f| f.name() == key) {
                    let dt = infer_type(val);
                    fields.push(Field::new(key, dt, true));
                }
            }
        }
    }

    if fields.is_empty() {
        return Err(PqError::Other("No fields found in JSON data".to_string()));
    }

    Ok(Schema::new(fields))
}

fn infer_type(value: &Value) -> DataType {
    match value {
        Value::Null => DataType::Utf8,
        Value::Bool(_) => DataType::Boolean,
        Value::Number(n) => {
            if n.is_i64() {
                DataType::Int64
            } else {
                DataType::Float64
            }
        }
        Value::String(_) => DataType::Utf8,
        Value::Array(arr) => {
            if let Some(first) = arr.first() {
                let inner = infer_type(first);
                DataType::List(Arc::new(Field::new("item", inner, true)))
            } else {
                DataType::List(Arc::new(Field::new("item", DataType::Utf8, true)))
            }
        }
        Value::Object(_) => DataType::Utf8, // Flatten nested objects to JSON strings
    }
}

pub fn json_values_to_batches(values: &[Value], schema: &Schema) -> Result<Vec<RecordBatch>> {
    let batch_size = 8192;
    let mut batches = Vec::new();

    for chunk in values.chunks(batch_size) {
        let mut columns: Vec<Arc<dyn Array>> = Vec::new();

        for field in schema.fields() {
            let array = build_array(field, chunk)?;
            columns.push(array);
        }

        let batch = RecordBatch::try_new(Arc::new(schema.clone()), columns)?;
        batches.push(batch);
    }

    Ok(batches)
}

fn build_array(field: &Field, values: &[Value]) -> Result<Arc<dyn Array>> {
    let name = field.name();
    match field.data_type() {
        DataType::Boolean => {
            let arr: BooleanArray = values
                .iter()
                .map(|v| v.get(name).and_then(|v| v.as_bool()))
                .collect();
            Ok(Arc::new(arr))
        }
        DataType::Int64 => {
            let arr: Int64Array = values
                .iter()
                .map(|v| v.get(name).and_then(|v| v.as_i64()))
                .collect();
            Ok(Arc::new(arr))
        }
        DataType::Float64 => {
            let arr: Float64Array = values
                .iter()
                .map(|v| v.get(name).and_then(|v| v.as_f64()))
                .collect();
            Ok(Arc::new(arr))
        }
        DataType::Utf8 => {
            let arr: StringArray = values
                .iter()
                .map(|v| {
                    v.get(name).and_then(|v| match v {
                        Value::String(s) => Some(s.as_str()),
                        Value::Null => None,
                        _other => None,
                    })
                })
                .collect();
            Ok(Arc::new(arr))
        }
        _ => {
            // Fallback: convert to string
            let arr: StringArray = values
                .iter()
                .map(|v| v.get(name).map(|v| v.to_string()))
                .collect();
            Ok(Arc::new(arr))
        }
    }
}
