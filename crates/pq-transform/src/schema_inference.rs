use arrow::array::*;
use arrow::buffer::OffsetBuffer;
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

    // Collect all keys from all objects, merging types across rows
    for value in values {
        if let Value::Object(obj) = value {
            for (key, val) in obj {
                if let Some(existing) = fields.iter_mut().find(|f| f.name() == key) {
                    // Widen the type if needed (e.g., Int64 + Float64 -> Float64)
                    let new_type = infer_type(val);
                    let merged = widen_types(existing.data_type(), &new_type);
                    if &merged != existing.data_type() {
                        *existing = Field::new(key, merged, true);
                    }
                } else {
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
        Value::Null => DataType::Null,
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
            if arr.is_empty() {
                DataType::List(Arc::new(Field::new("item", DataType::Null, true)))
            } else {
                // Infer from all elements and widen
                let mut inner = infer_type(&arr[0]);
                for elem in &arr[1..] {
                    inner = widen_types(&inner, &infer_type(elem));
                }
                DataType::List(Arc::new(Field::new("item", inner, true)))
            }
        }
        Value::Object(obj) => {
            let fields: Vec<Field> = obj
                .iter()
                .map(|(k, v)| Field::new(k, infer_type(v), true))
                .collect();
            DataType::Struct(Fields::from(fields))
        }
    }
}

/// Widen two types to a common type that can represent both.
fn widen_types(a: &DataType, b: &DataType) -> DataType {
    if a == b {
        return a.clone();
    }
    // Null can be widened to anything
    if *a == DataType::Null {
        return b.clone();
    }
    if *b == DataType::Null {
        return a.clone();
    }
    // Int64 + Float64 -> Float64
    match (a, b) {
        (DataType::Int64, DataType::Float64) | (DataType::Float64, DataType::Int64) => {
            DataType::Float64
        }
        // List widening: merge inner types
        (DataType::List(a_inner), DataType::List(b_inner)) => {
            let inner = widen_types(a_inner.data_type(), b_inner.data_type());
            DataType::List(Arc::new(Field::new("item", inner, true)))
        }
        // Struct widening: union of fields
        (DataType::Struct(a_fields), DataType::Struct(b_fields)) => {
            let mut fields: Vec<Field> = a_fields
                .iter()
                .map(|f| f.as_ref().clone())
                .collect();
            for b_field in b_fields.iter() {
                if let Some(existing) = fields.iter_mut().find(|f| f.name() == b_field.name()) {
                    let merged = widen_types(existing.data_type(), b_field.data_type());
                    *existing = Field::new(existing.name(), merged, true);
                } else {
                    fields.push(b_field.as_ref().clone());
                }
            }
            DataType::Struct(Fields::from(fields))
        }
        // Fallback: stringify
        _ => DataType::Utf8,
    }
}

pub fn json_values_to_batches(values: &[Value], schema: &Schema) -> Result<Vec<RecordBatch>> {
    let batch_size = 8192;
    let mut batches = Vec::new();

    for chunk in values.chunks(batch_size) {
        let mut columns: Vec<Arc<dyn Array>> = Vec::new();

        for field in schema.fields() {
            let array = build_array(field.name(), field.data_type(), chunk)?;
            columns.push(array);
        }

        let batch = RecordBatch::try_new(Arc::new(schema.clone()), columns)?;
        batches.push(batch);
    }

    Ok(batches)
}

/// Build an Arrow array for a column from JSON values.
/// `name` is the key to look up in each row object.
fn build_array(name: &str, dt: &DataType, values: &[Value]) -> Result<Arc<dyn Array>> {
    match dt {
        DataType::Null => Ok(Arc::new(NullArray::new(values.len()))),
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
                        _ => None,
                    })
                })
                .collect();
            Ok(Arc::new(arr))
        }
        DataType::Struct(fields) => {
            build_struct_array(name, fields, values)
        }
        DataType::List(inner_field) => {
            build_list_array(name, inner_field, values)
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

/// Build a StructArray from JSON objects.
fn build_struct_array(
    name: &str,
    fields: &Fields,
    values: &[Value],
) -> Result<Arc<dyn Array>> {
    // Extract the nested object for each row
    let nested_values: Vec<Option<&Value>> = values
        .iter()
        .map(|v| v.get(name))
        .collect();

    let null_buffer: Vec<bool> = nested_values.iter().map(|v| v.is_some()).collect();

    let child_arrays: Vec<Arc<dyn Array>> = fields
        .iter()
        .map(|field| {
            build_struct_child_array(field.name(), field.data_type(), &nested_values)
        })
        .collect::<Result<_>>()?;

    let struct_array = StructArray::try_new(
        fields.clone(),
        child_arrays,
        Some(null_buffer.into()),
    )?;
    Ok(Arc::new(struct_array))
}

/// Build a child array of a struct from extracted parent values.
fn build_struct_child_array(
    name: &str,
    dt: &DataType,
    parent_values: &[Option<&Value>],
) -> Result<Arc<dyn Array>> {
    match dt {
        DataType::Null => Ok(Arc::new(NullArray::new(parent_values.len()))),
        DataType::Boolean => {
            let arr: BooleanArray = parent_values
                .iter()
                .map(|v| v.and_then(|obj| obj.get(name)).and_then(|v| v.as_bool()))
                .collect();
            Ok(Arc::new(arr))
        }
        DataType::Int64 => {
            let arr: Int64Array = parent_values
                .iter()
                .map(|v| v.and_then(|obj| obj.get(name)).and_then(|v| v.as_i64()))
                .collect();
            Ok(Arc::new(arr))
        }
        DataType::Float64 => {
            let arr: Float64Array = parent_values
                .iter()
                .map(|v| v.and_then(|obj| obj.get(name)).and_then(|v| v.as_f64()))
                .collect();
            Ok(Arc::new(arr))
        }
        DataType::Utf8 => {
            let arr: StringArray = parent_values
                .iter()
                .map(|v| {
                    v.and_then(|obj| obj.get(name))
                        .and_then(|v| v.as_str())
                })
                .collect();
            Ok(Arc::new(arr))
        }
        DataType::Struct(child_fields) => {
            // Recurse: extract the nested object from each parent
            let child_values: Vec<Option<&Value>> = parent_values
                .iter()
                .map(|v| v.and_then(|obj| obj.get(name)))
                .collect();

            let null_buffer: Vec<bool> = child_values.iter().map(|v| v.is_some()).collect();

            let child_arrays: Vec<Arc<dyn Array>> = child_fields
                .iter()
                .map(|field| {
                    build_struct_child_array(field.name(), field.data_type(), &child_values)
                })
                .collect::<Result<_>>()?;

            let struct_array = StructArray::try_new(
                child_fields.clone(),
                child_arrays,
                Some(null_buffer.into()),
            )?;
            Ok(Arc::new(struct_array))
        }
        DataType::List(inner_field) => {
            build_list_child_array(name, inner_field, parent_values)
        }
        _ => {
            // Fallback: convert to string
            let arr: StringArray = parent_values
                .iter()
                .map(|v| v.and_then(|obj| obj.get(name)).map(|v| v.to_string()))
                .collect();
            Ok(Arc::new(arr))
        }
    }
}

/// Build a ListArray from JSON arrays at the top level.
fn build_list_array(
    name: &str,
    inner_field: &Arc<Field>,
    values: &[Value],
) -> Result<Arc<dyn Array>> {
    let parent_values: Vec<Option<&Value>> = values.iter().map(|v| v.get(name)).collect();
    build_list_child_array(name, inner_field, &parent_values)
}

/// Build a ListArray from pre-extracted parent values.
fn build_list_child_array(
    name: &str,
    inner_field: &Arc<Field>,
    parent_values: &[Option<&Value>],
) -> Result<Arc<dyn Array>> {
    let _ = name; // name was used to extract from parent; here we work with already-extracted values
    // Build offsets and flatten all list elements
    let mut offsets: Vec<i32> = Vec::with_capacity(parent_values.len() + 1);
    let mut flat_elements: Vec<Value> = Vec::new();
    offsets.push(0);

    for val in parent_values {
        match val {
            Some(Value::Array(arr)) => {
                flat_elements.extend(arr.iter().cloned());
                offsets.push(flat_elements.len() as i32);
            }
            _ => {
                offsets.push(flat_elements.len() as i32);
            }
        }
    }

    let null_buffer: Vec<bool> = parent_values
        .iter()
        .map(|v| matches!(v, Some(Value::Array(_))))
        .collect();

    // Build the child values array
    let child_array = build_flat_array(inner_field.data_type(), &flat_elements)?;

    let list_array = ListArray::try_new(
        inner_field.clone(),
        OffsetBuffer::new(offsets.into()),
        child_array,
        Some(null_buffer.into()),
    )?;
    Ok(Arc::new(list_array))
}

/// Build an array from a flat vector of JSON values (no field name lookup).
fn build_flat_array(dt: &DataType, values: &[Value]) -> Result<Arc<dyn Array>> {
    match dt {
        DataType::Null => Ok(Arc::new(NullArray::new(values.len()))),
        DataType::Boolean => {
            let arr: BooleanArray = values.iter().map(|v| v.as_bool()).collect();
            Ok(Arc::new(arr))
        }
        DataType::Int64 => {
            let arr: Int64Array = values.iter().map(|v| v.as_i64()).collect();
            Ok(Arc::new(arr))
        }
        DataType::Float64 => {
            let arr: Float64Array = values.iter().map(|v| v.as_f64()).collect();
            Ok(Arc::new(arr))
        }
        DataType::Utf8 => {
            let arr: StringArray = values.iter().map(|v| v.as_str()).collect();
            Ok(Arc::new(arr))
        }
        DataType::Struct(fields) => {
            // Each value should be a JSON object
            let null_buffer: Vec<bool> = values.iter().map(|v| v.is_object()).collect();

            let child_arrays: Vec<Arc<dyn Array>> = fields
                .iter()
                .map(|field| {
                    build_flat_struct_child(field.name(), field.data_type(), values)
                })
                .collect::<Result<_>>()?;

            let struct_array = StructArray::try_new(
                fields.clone(),
                child_arrays,
                Some(null_buffer.into()),
            )?;
            Ok(Arc::new(struct_array))
        }
        DataType::List(inner_field) => {
            // Each value should be a JSON array
            let mut offsets: Vec<i32> = Vec::with_capacity(values.len() + 1);
            let mut flat_elements: Vec<Value> = Vec::new();
            offsets.push(0);

            let null_buffer: Vec<bool> = values.iter().map(|v| v.is_array()).collect();

            for val in values {
                if let Value::Array(arr) = val {
                    flat_elements.extend(arr.iter().cloned());
                }
                offsets.push(flat_elements.len() as i32);
            }

            let child_array = build_flat_array(inner_field.data_type(), &flat_elements)?;

            let list_array = ListArray::try_new(
                inner_field.clone(),
                OffsetBuffer::new(offsets.into()),
                child_array,
                Some(null_buffer.into()),
            )?;
            Ok(Arc::new(list_array))
        }
        _ => {
            // Fallback: stringify
            let arr: StringArray = values
                .iter()
                .map(|v| match v {
                    Value::String(s) => Some(s.as_str()),
                    Value::Null => None,
                    _ => None,
                })
                .collect();
            Ok(Arc::new(arr))
        }
    }
}

/// Build a child array for a struct field from flat JSON values.
fn build_flat_struct_child(
    name: &str,
    dt: &DataType,
    values: &[Value],
) -> Result<Arc<dyn Array>> {
    match dt {
        DataType::Null => Ok(Arc::new(NullArray::new(values.len()))),
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
                .map(|v| v.get(name).and_then(|v| v.as_str()))
                .collect();
            Ok(Arc::new(arr))
        }
        DataType::Struct(fields) => {
            let child_values: Vec<Option<&Value>> = values
                .iter()
                .map(|v| v.get(name))
                .collect();

            let null_buffer: Vec<bool> = child_values.iter().map(|v| v.is_some()).collect();

            let child_arrays: Vec<Arc<dyn Array>> = fields
                .iter()
                .map(|field| {
                    build_struct_child_array(field.name(), field.data_type(), &child_values)
                })
                .collect::<Result<_>>()?;

            let struct_array = StructArray::try_new(
                fields.clone(),
                child_arrays,
                Some(null_buffer.into()),
            )?;
            Ok(Arc::new(struct_array))
        }
        DataType::List(inner_field) => {
            let parent_values: Vec<Option<&Value>> = values.iter().map(|v| v.get(name)).collect();
            build_list_child_array(name, inner_field, &parent_values)
        }
        _ => {
            let arr: StringArray = values
                .iter()
                .map(|v| v.get(name).map(|v| v.to_string()))
                .collect();
            Ok(Arc::new(arr))
        }
    }
}
