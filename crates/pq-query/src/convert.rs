use arrow::array::*;
use arrow::datatypes::*;
use serde_json::Value;

/// Convert a single row from a RecordBatch to a JSON object
pub fn batch_row_to_json(batch: &RecordBatch, row_idx: usize) -> Value {
    let schema = batch.schema();
    let columns = resolve_columns(batch);
    let mut obj = serde_json::Map::new();
    for (col_idx, field) in schema.fields().iter().enumerate() {
        let value = array_value_to_json(columns[col_idx].as_ref(), row_idx);
        obj.insert(field.name().clone(), value);
    }
    Value::Object(obj)
}

/// Convert a RecordBatch to a vector of JSON objects (one per row)
pub fn batch_to_json_rows(batch: &RecordBatch) -> Vec<Value> {
    let mut rows = Vec::with_capacity(batch.num_rows());
    let schema = batch.schema();
    // Resolve each column once per batch (not once per row). Dictionary
    // columns in particular require an `arrow::compute::cast` over the
    // whole array to unpack; doing that inside the per-row loop turns an
    // O(rows) conversion into O(rows^2) — one full-array cast per row.
    let columns = resolve_columns(batch);

    for row_idx in 0..batch.num_rows() {
        let mut obj = serde_json::Map::new();
        for (col_idx, field) in schema.fields().iter().enumerate() {
            let value = array_value_to_json(columns[col_idx].as_ref(), row_idx);
            obj.insert(field.name().clone(), value);
        }
        rows.push(Value::Object(obj));
    }
    rows
}

/// Resolve each top-level column of a batch once, unpacking dictionary
/// columns to their value type. This is the hoisted, once-per-batch
/// counterpart of the per-row dictionary handling in `array_value_to_json`
/// (which remains as a fallback for dictionaries nested inside lists,
/// structs, or maps, where it runs once per element rather than once per
/// row of the whole batch).
fn resolve_columns(batch: &RecordBatch) -> Vec<ArrayRef> {
    batch
        .columns()
        .iter()
        .map(|col| match col.data_type() {
            DataType::Dictionary(_, value_type) => {
                match arrow::compute::kernels::cast::cast(col.as_ref(), value_type.as_ref()) {
                    Ok(unpacked) => unpacked,
                    Err(_) => col.clone(),
                }
            }
            _ => col.clone(),
        })
        .collect()
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
            Value::String(format_decimal_string(v.to_string(), *scale as u32))
        }
        DataType::Decimal256(_, scale) => {
            let arr = array.as_any().downcast_ref::<Decimal256Array>().unwrap();
            let v = arr.value(idx);
            Value::String(format_decimal_string(v.to_string(), *scale as u32))
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
            let arr = array.as_any().downcast_ref::<FixedSizeListArray>().unwrap();
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

/// Render a decimal's unscaled integer (as produced by `Display` on the
/// storage type, e.g. `i128` or arrow's `i256`) with the decimal point
/// placed `scale` digits from the right, using exactly `scale` fractional
/// digits (never trimmed). This works entirely on the digit string, so it
/// is exact for any width and never routes through a float.
///
/// Examples (scale=2): "12345" -> "123.45", "12300" -> "123.00",
/// "0" -> "0.00", "-4567" -> "-45.67". scale=0 returns the input unchanged.
fn format_decimal_string(raw: String, scale: u32) -> String {
    if scale == 0 {
        return raw;
    }
    let scale = scale as usize;
    let negative = raw.starts_with('-');
    let digits = if negative { &raw[1..] } else { &raw[..] };
    let padded;
    let digits = if digits.len() <= scale {
        padded = format!("{}{}", "0".repeat(scale - digits.len() + 1), digits);
        padded.as_str()
    } else {
        digits
    };
    let point = digits.len() - scale;
    let (int_part, frac_part) = digits.split_at(point);
    format!("{}{int_part}.{frac_part}", if negative { "-" } else { "" })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    // ---- format_decimal_string: exercises the class of bug directly, on
    // the exact function that replaced the broken one-liner. Ground truth
    // for every case here was cross-checked against pyarrow separately
    // (dec.parquet fixture built with pyarrow 21, decimal128(38,2) and
    // decimal256(40,2)).

    #[test]
    fn scale_zero_is_unchanged() {
        // Control: a correct implementation must pass this too, and so did
        // the old code by coincidence (scale==0 was already special-cased).
        assert_eq!(format_decimal_string("123".into(), 0), "123");
        assert_eq!(format_decimal_string("-123".into(), 0), "-123");
        assert_eq!(format_decimal_string("0".into(), 0), "0");
    }

    #[test]
    fn ordinary_scale_places_the_point_correctly() {
        // pyarrow ground truth: decimal(_, 2) unscaled 12345 -> "123.45".
        assert_eq!(format_decimal_string("12345".into(), 2), "123.45");
    }

    #[test]
    fn trailing_zero_fraction_is_not_trimmed() {
        // pyarrow ground truth: unscaled 12300, scale 2 -> "123.00", not
        // "123". The buggy line trimmed trailing zeros off the mantissa
        // before applying scale, which silently dropped these digits.
        assert_eq!(format_decimal_string("12300".into(), 2), "123.00");
    }

    #[test]
    fn distinct_values_do_not_collapse() {
        // pyarrow ground truth: 1.23 and 123.00 are different numbers and
        // must render differently. The buggy line rendered both as
        // "123.2" (scale appended as if it were fractional digits, with
        // trailing zeros trimmed off first) -- an unrecoverable collision.
        let a = format_decimal_string("123".into(), 2); // 1.23
        let b = format_decimal_string("12300".into(), 2); // 123.00
        assert_eq!(a, "1.23");
        assert_eq!(b, "123.00");
        assert_ne!(a, b);
    }

    #[test]
    fn zero_renders_with_full_fractional_width() {
        assert_eq!(format_decimal_string("0".into(), 2), "0.00");
    }

    #[test]
    fn negative_values_keep_sign_outside_the_point() {
        // pyarrow ground truth: unscaled -4567, scale 2 -> "-45.67".
        assert_eq!(format_decimal_string("-4567".into(), 2), "-45.67");
        // Negative value smaller in magnitude than the scale still pads.
        assert_eq!(format_decimal_string("-7".into(), 2), "-0.07");
    }

    #[test]
    fn large_scale_pads_with_leading_zeros() {
        assert_eq!(format_decimal_string("5".into(), 6), "0.000005");
    }

    #[test]
    fn beyond_f64_exact_integer_range_stays_exact() {
        // pyarrow ground truth: decimal(38,2) unscaled
        // 1234567890123456789 (19 digits) -> "12345678901234567.89".
        // f64 can only represent integers exactly up to 2^53 (~9.007e15);
        // the old Decimal128 arm routed through `v as f64` and lost the
        // cents here, rendering 1.2345678901234568e+16.
        assert_eq!(
            format_decimal_string("1234567890123456789".into(), 2),
            "12345678901234567.89"
        );
    }

    #[test]
    fn decimal128_max_precision_is_exact() {
        // decimal128(38, 2), unscaled value is 38 nines.
        let unscaled = "9".repeat(38);
        let expected = format!("{}.99", "9".repeat(36));
        assert_eq!(format_decimal_string(unscaled, 2), expected);
    }

    #[test]
    fn decimal256_max_precision_is_exact() {
        // decimal256(76, 10), unscaled value is 76 nines -- the maximum
        // precision and scale Decimal256 allows. Confirms the digit-string
        // approach has no width limit (unlike routing through f64/i128).
        let unscaled = "9".repeat(76);
        let expected = format!("{}.{}", "9".repeat(66), "9".repeat(10));
        assert_eq!(format_decimal_string(unscaled, 10), expected);
    }

    // ---- Integration: array_value_to_json end to end, for both decimal
    // widths, via real Arrow arrays (not just the helper function), plus
    // the output-type decision (string, not number, for both widths).

    fn decimal128_json(values: Vec<i128>, precision: u8, scale: i8, idx: usize) -> Value {
        let arr = Decimal128Array::from(values)
            .with_precision_and_scale(precision, scale)
            .unwrap();
        array_value_to_json(&arr, idx)
    }

    fn decimal256_json(values: Vec<i256>, precision: u8, scale: i8, idx: usize) -> Value {
        let arr = Decimal256Array::from(values)
            .with_precision_and_scale(precision, scale)
            .unwrap();
        array_value_to_json(&arr, idx)
    }

    #[test]
    fn decimal128_end_to_end_matches_pyarrow_ground_truth() {
        // Same five cases as the CLI repro against dec.parquet
        // (decimal128(38,2)): 123.45, 1.23, 123.00, 0.00, -45.67.
        let vals = vec![12345i128, 123, 12300, 0, -4567];
        let expected = ["123.45", "1.23", "123.00", "0.00", "-45.67"];
        for (i, exp) in expected.iter().enumerate() {
            assert_eq!(
                decimal128_json(vals.clone(), 38, 2, i),
                Value::String((*exp).to_string()),
                "row {i}"
            );
        }
    }

    #[test]
    fn decimal256_end_to_end_matches_pyarrow_ground_truth() {
        // Same cases, decimal256(40,2): 123.45, 1.23, 123.00, 0.00, -45.67.
        let vals: Vec<i256> = ["12345", "123", "12300", "0", "-4567"]
            .iter()
            .map(|s| s.parse::<i256>().unwrap())
            .collect();
        let expected = ["123.45", "1.23", "123.00", "0.00", "-45.67"];
        for (i, exp) in expected.iter().enumerate() {
            assert_eq!(
                decimal256_json(vals.clone(), 40, 2, i),
                Value::String((*exp).to_string()),
                "row {i}"
            );
        }
    }

    #[test]
    fn decimal128_and_decimal256_are_interchangeable_for_equal_values() {
        // Output-type decision: both widths must emit the same JSON *type*
        // (String) and the same text for the same logical value, so a
        // machine consumer never has to special-case which decimal width
        // produced a field.
        let d128 = decimal128_json(vec![1234567890123456789i128], 38, 2, 0);
        let d256 = decimal256_json(vec!["1234567890123456789".parse().unwrap()], 40, 2, 0);
        assert_eq!(d128, d256);
        assert_eq!(d128, Value::String("12345678901234567.89".into()));
    }

    #[test]
    fn decimal128_output_is_a_json_string_not_a_lossy_number() {
        // This is the deliberate, documented output-type change: Decimal128
        // used to emit a JSON number (`v as f64 / 10^scale`), which loses
        // precision beyond f64's exact integer range. It now emits a
        // string with exact digits, matching Decimal256 and matching `-f
        // csv`/`-f table` (which already go through arrow's exact
        // ArrayFormatter).
        let v = decimal128_json(vec![1234567890123456789i128], 38, 2, 0);
        match v {
            Value::String(s) => assert_eq!(s, "12345678901234567.89"),
            other => panic!("expected exact string, got {other:?} (lossy number?)"),
        }
    }

    // ---- Dictionary column: correctness after hoisting the cast out of
    // the per-row loop. (The O(n^2) -> O(n) improvement itself is measured
    // at the CLI level with a real binary and wall-clock/instruction
    // counts, not asserted here -- a unit test can't establish a
    // performance claim.)

    #[test]
    fn dictionary_column_decodes_correctly_via_batch_to_json_rows() {
        let dict: DictionaryArray<Int32Type> = vec!["red", "green", "red", "blue", "green"]
            .into_iter()
            .collect();
        let schema = Arc::new(Schema::new(vec![Field::new(
            "color",
            dict.data_type().clone(),
            false,
        )]));
        let batch = RecordBatch::try_new(schema, vec![Arc::new(dict)]).unwrap();

        let rows = batch_to_json_rows(&batch);
        let colors: Vec<&str> = rows.iter().map(|r| r["color"].as_str().unwrap()).collect();
        assert_eq!(colors, vec!["red", "green", "red", "blue", "green"]);
    }

    #[test]
    fn dictionary_column_decodes_correctly_via_batch_row_to_json() {
        let dict: DictionaryArray<Int32Type> = vec!["red", "green", "blue"].into_iter().collect();
        let schema = Arc::new(Schema::new(vec![Field::new(
            "color",
            dict.data_type().clone(),
            false,
        )]));
        let batch = RecordBatch::try_new(schema, vec![Arc::new(dict)]).unwrap();

        for (i, expected) in ["red", "green", "blue"].iter().enumerate() {
            let row = batch_row_to_json(&batch, i);
            assert_eq!(row["color"].as_str().unwrap(), *expected);
        }
    }
}
