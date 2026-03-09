use arrow::array::*;
use arrow::datatypes::*;
use serde_json::Value;

/// Convert a single row from a RecordBatch to a JSON object
pub fn batch_row_to_json(batch: &RecordBatch, row_idx: usize) -> Value {
    let schema = batch.schema();
    let mut obj = serde_json::Map::new();
    for (col_idx, field) in schema.fields().iter().enumerate() {
        let col = batch.column(col_idx);
        let value = array_value_to_json(col.as_ref(), row_idx);
        obj.insert(field.name().clone(), value);
    }
    Value::Object(obj)
}

/// Convert a RecordBatch to a vector of JSON objects (one per row)
pub fn batch_to_json_rows(batch: &RecordBatch) -> Vec<Value> {
    let mut rows = Vec::with_capacity(batch.num_rows());
    let schema = batch.schema();

    for row_idx in 0..batch.num_rows() {
        let mut obj = serde_json::Map::new();
        for (col_idx, field) in schema.fields().iter().enumerate() {
            let col = batch.column(col_idx);
            let value = array_value_to_json(col.as_ref(), row_idx);
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
        DataType::FixedSizeBinary(_) => {
            let arr = array
                .as_any()
                .downcast_ref::<FixedSizeBinaryArray>()
                .unwrap();
            Value::String(hex_encode(arr.value(idx)))
        }
        DataType::Date32 => {
            let arr = array.as_any().downcast_ref::<Date32Array>().unwrap();
            let epoch_days = arr.value(idx) as i64;
            let dt = chrono::NaiveDate::from_num_days_from_ce_opt(
                epoch_days as i32 + 719_163, // Unix epoch = day 719163 in CE
            );
            match dt {
                Some(d) => Value::String(d.format("%Y-%m-%d").to_string()),
                None => Value::Number(serde_json::Number::from(epoch_days)),
            }
        }
        DataType::Date64 => {
            let arr = array.as_any().downcast_ref::<Date64Array>().unwrap();
            let epoch_ms = arr.value(idx);
            let dt = chrono::DateTime::from_timestamp_millis(epoch_ms);
            match dt {
                Some(d) => Value::String(d.format("%Y-%m-%d").to_string()),
                None => Value::Number(serde_json::Number::from(epoch_ms)),
            }
        }
        DataType::Timestamp(unit, _tz) => {
            let val = match unit {
                TimeUnit::Second => {
                    let arr = array
                        .as_any()
                        .downcast_ref::<TimestampSecondArray>()
                        .unwrap();
                    chrono::DateTime::from_timestamp(arr.value(idx), 0)
                }
                TimeUnit::Millisecond => {
                    let arr = array
                        .as_any()
                        .downcast_ref::<TimestampMillisecondArray>()
                        .unwrap();
                    chrono::DateTime::from_timestamp_millis(arr.value(idx))
                }
                TimeUnit::Microsecond => {
                    let arr = array
                        .as_any()
                        .downcast_ref::<TimestampMicrosecondArray>()
                        .unwrap();
                    chrono::DateTime::from_timestamp_micros(arr.value(idx))
                }
                TimeUnit::Nanosecond => {
                    let arr = array
                        .as_any()
                        .downcast_ref::<TimestampNanosecondArray>()
                        .unwrap();
                    let nanos = arr.value(idx);
                    chrono::DateTime::from_timestamp(
                        nanos / 1_000_000_000,
                        (nanos % 1_000_000_000) as u32,
                    )
                }
            };
            match val {
                Some(dt) => Value::String(dt.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)),
                None => Value::Null,
            }
        }
        DataType::Decimal128(_, scale) => {
            let arr = array.as_any().downcast_ref::<Decimal128Array>().unwrap();
            let v = arr.value(idx);
            let scale = *scale as u32;
            if scale == 0 {
                Value::String(v.to_string())
            } else {
                let f = v as f64 / 10f64.powi(scale as i32);
                serde_json::Number::from_f64(f)
                    .map(Value::Number)
                    .unwrap_or(Value::String(format!("{f}")))
            }
        }
        DataType::Decimal256(_, scale) => {
            let arr = array.as_any().downcast_ref::<Decimal256Array>().unwrap();
            let v = arr.value(idx);
            let scale = *scale as u32;
            if scale == 0 {
                Value::String(v.to_string())
            } else {
                Value::String(format!(
                    "{}.{}",
                    v.to_string().trim_end_matches('0'),
                    scale
                ))
            }
        }
        DataType::List(_) => {
            let arr = array.as_any().downcast_ref::<ListArray>().unwrap();
            let values = arr.value(idx);
            let items: Vec<Value> = (0..values.len())
                .map(|i| array_value_to_json(values.as_ref(), i))
                .collect();
            Value::Array(items)
        }
        DataType::LargeList(_) => {
            let arr = array.as_any().downcast_ref::<LargeListArray>().unwrap();
            let values = arr.value(idx);
            let items: Vec<Value> = (0..values.len())
                .map(|i| array_value_to_json(values.as_ref(), i))
                .collect();
            Value::Array(items)
        }
        DataType::FixedSizeList(_, _) => {
            let arr = array
                .as_any()
                .downcast_ref::<FixedSizeListArray>()
                .unwrap();
            let values = arr.value(idx);
            let items: Vec<Value> = (0..values.len())
                .map(|i| array_value_to_json(values.as_ref(), i))
                .collect();
            Value::Array(items)
        }
        DataType::Struct(_) => {
            let arr = array.as_any().downcast_ref::<StructArray>().unwrap();
            let mut obj = serde_json::Map::new();
            for (i, field) in arr.fields().iter().enumerate() {
                let col = arr.column(i);
                obj.insert(field.name().clone(), array_value_to_json(col.as_ref(), idx));
            }
            Value::Object(obj)
        }
        DataType::Map(_, _) => {
            let arr = array.as_any().downcast_ref::<MapArray>().unwrap();
            let entry = arr.value(idx);
            // Map entries are stored as a struct with "key" and "value" fields
            let keys = entry.column(0);
            let vals = entry.column(1);
            let mut obj = serde_json::Map::new();
            for i in 0..entry.len() {
                let key = array_value_to_json(keys.as_ref(), i);
                let val = array_value_to_json(vals.as_ref(), i);
                // Use string representation of key
                let key_str = match key {
                    Value::String(s) => s,
                    other => other.to_string(),
                };
                obj.insert(key_str, val);
            }
            Value::Object(obj)
        }
        DataType::Dictionary(_, _) => {
            // Unpack dictionary to its actual values
            let dict_arr = arrow::compute::kernels::cast::cast(
                array,
                match array.data_type() {
                    DataType::Dictionary(_, v) => v.as_ref(),
                    _ => unreachable!(),
                },
            );
            match dict_arr {
                Ok(unpacked) => array_value_to_json(unpacked.as_ref(), idx),
                Err(_) => {
                    // Fallback: use display formatting
                    format_fallback(array, idx)
                }
            }
        }
        _ => format_fallback(array, idx),
    }
}

fn format_fallback(array: &dyn Array, idx: usize) -> Value {
    let formatted = arrow::util::display::ArrayFormatter::try_new(array, &Default::default());
    match formatted {
        Ok(f) => Value::String(f.value(idx).to_string()),
        Err(_) => Value::String(format!("<unsupported:{:?}>", array.data_type())),
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
