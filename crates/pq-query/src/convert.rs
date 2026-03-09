use arrow::array::*;
use arrow::datatypes::*;
use serde_json::Value;

/// Convert a RecordBatch to a vector of JSON objects (one per row)
pub fn batch_to_json_rows(batch: &RecordBatch) -> Vec<Value> {
    let mut rows = Vec::with_capacity(batch.num_rows());
    let schema = batch.schema();

    for row_idx in 0..batch.num_rows() {
        let mut obj = serde_json::Map::new();
        for (col_idx, field) in schema.fields().iter().enumerate() {
            let col = batch.column(col_idx);
            let value = array_value_to_json(col, row_idx);
            obj.insert(field.name().clone(), value);
        }
        rows.push(Value::Object(obj));
    }
    rows
}

fn array_value_to_json(array: &dyn Array, idx: usize) -> Value {
    if array.is_null(idx) {
        return Value::Null;
    }

    match array.data_type() {
        DataType::Null => Value::Null,
        DataType::Boolean => {
            let arr = array.as_any().downcast_ref::<BooleanArray>().unwrap();
            Value::Bool(arr.value(idx))
        }
        DataType::Int8 => {
            let arr = array.as_any().downcast_ref::<Int8Array>().unwrap();
            Value::Number(serde_json::Number::from(arr.value(idx) as i64))
        }
        DataType::Int16 => {
            let arr = array.as_any().downcast_ref::<Int16Array>().unwrap();
            Value::Number(serde_json::Number::from(arr.value(idx) as i64))
        }
        DataType::Int32 => {
            let arr = array.as_any().downcast_ref::<Int32Array>().unwrap();
            Value::Number(serde_json::Number::from(arr.value(idx) as i64))
        }
        DataType::Int64 => {
            let arr = array.as_any().downcast_ref::<Int64Array>().unwrap();
            Value::Number(serde_json::Number::from(arr.value(idx)))
        }
        DataType::UInt8 => {
            let arr = array.as_any().downcast_ref::<UInt8Array>().unwrap();
            Value::Number(serde_json::Number::from(arr.value(idx) as u64))
        }
        DataType::UInt16 => {
            let arr = array.as_any().downcast_ref::<UInt16Array>().unwrap();
            Value::Number(serde_json::Number::from(arr.value(idx) as u64))
        }
        DataType::UInt32 => {
            let arr = array.as_any().downcast_ref::<UInt32Array>().unwrap();
            Value::Number(serde_json::Number::from(arr.value(idx) as u64))
        }
        DataType::UInt64 => {
            let arr = array.as_any().downcast_ref::<UInt64Array>().unwrap();
            Value::Number(serde_json::Number::from(arr.value(idx)))
        }
        DataType::Float32 => {
            let arr = array.as_any().downcast_ref::<Float32Array>().unwrap();
            let v = arr.value(idx) as f64;
            serde_json::Number::from_f64(v)
                .map(Value::Number)
                .unwrap_or(Value::Null)
        }
        DataType::Float64 => {
            let arr = array.as_any().downcast_ref::<Float64Array>().unwrap();
            let v = arr.value(idx);
            serde_json::Number::from_f64(v)
                .map(Value::Number)
                .unwrap_or(Value::Null)
        }
        DataType::Utf8 => {
            let arr = array.as_any().downcast_ref::<StringArray>().unwrap();
            Value::String(arr.value(idx).to_string())
        }
        DataType::LargeUtf8 => {
            let arr = array.as_any().downcast_ref::<LargeStringArray>().unwrap();
            Value::String(arr.value(idx).to_string())
        }
        DataType::Binary => {
            let arr = array.as_any().downcast_ref::<BinaryArray>().unwrap();
            Value::String(hex_encode(arr.value(idx)))
        }
        DataType::LargeBinary => {
            let arr = array.as_any().downcast_ref::<LargeBinaryArray>().unwrap();
            Value::String(hex_encode(arr.value(idx)))
        }
        DataType::Decimal128(_, scale) => {
            let arr = array.as_any().downcast_ref::<Decimal128Array>().unwrap();
            let v = arr.value(idx);
            let scale = *scale as u32;
            if scale == 0 {
                // i128 doesn't impl Into<serde_json::Number>, so format as string for large values
                Value::String(v.to_string())
            } else {
                let f = v as f64 / 10f64.powi(scale as i32);
                serde_json::Number::from_f64(f)
                    .map(Value::Number)
                    .unwrap_or(Value::String(format!("{f}")))
            }
        }
        DataType::List(_) => {
            let arr = array.as_any().downcast_ref::<ListArray>().unwrap();
            let values = arr.value(idx);
            let items: Vec<Value> = (0..values.len())
                .map(|i| array_value_to_json(&values, i))
                .collect();
            Value::Array(items)
        }
        DataType::LargeList(_) => {
            let arr = array.as_any().downcast_ref::<LargeListArray>().unwrap();
            let values = arr.value(idx);
            let items: Vec<Value> = (0..values.len())
                .map(|i| array_value_to_json(&values, i))
                .collect();
            Value::Array(items)
        }
        DataType::Struct(_) => {
            let arr = array.as_any().downcast_ref::<StructArray>().unwrap();
            let mut obj = serde_json::Map::new();
            for (i, field) in arr.fields().iter().enumerate() {
                let col = arr.column(i);
                obj.insert(field.name().clone(), array_value_to_json(col, idx));
            }
            Value::Object(obj)
        }
        _ => {
            // Fallback: use arrow's display formatting
            let formatted =
                arrow::util::display::ArrayFormatter::try_new(array, &Default::default());
            match formatted {
                Ok(f) => Value::String(f.value(idx).to_string()),
                Err(_) => Value::String(format!("<unsupported:{:?}>", array.data_type())),
            }
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(s, "{byte:02x}").unwrap();
    }
    s
}
