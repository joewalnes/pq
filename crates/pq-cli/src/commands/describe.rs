use std::io::Write;

use arrow::array::*;
use arrow::datatypes::*;

use crate::output::Format;

#[derive(Debug, serde::Serialize)]
pub struct ColumnDescription {
    pub column: String,
    pub dtype: String,
    pub count: usize,
    pub nulls: usize,
    pub null_pct: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mean: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stddev: Option<f64>,
    pub distinct: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top: Option<Vec<FreqEntry>>,
}

#[derive(Debug, serde::Serialize)]
pub struct FreqEntry {
    pub value: serde_json::Value,
    pub count: usize,
}

pub fn run(files: &[String], top_k: usize, format: Format) -> anyhow::Result<()> {
    let opts = pq_core::reader::ReadOptions::default();

    // Collect all batches
    let mut all_batches: Vec<RecordBatch> = Vec::new();
    for f in files {
        let (batches, _schema) = pq_core::reader::open_batches(f, &opts)?;
        all_batches.extend(batches);
    }

    if all_batches.is_empty() {
        anyhow::bail!("No data found");
    }

    let schema = all_batches[0].schema();
    let total_rows: usize = all_batches.iter().map(|b| b.num_rows()).sum();

    // Concatenate all arrays per column
    let mut descriptions: Vec<ColumnDescription> = Vec::new();

    for (col_idx, field) in schema.fields().iter().enumerate() {
        let arrays: Vec<&dyn Array> = all_batches
            .iter()
            .map(|b| b.column(col_idx).as_ref())
            .collect();
        let concatenated = arrow::compute::concat(&arrays)?;

        let null_count = concatenated.null_count();
        let null_pct = if total_rows > 0 {
            (null_count as f64 / total_rows as f64) * 100.0
        } else {
            0.0
        };

        let (min, max, mean, stddev) = compute_numeric_stats(concatenated.as_ref());
        let (distinct, top) = compute_distinct_top_k(concatenated.as_ref(), top_k);

        descriptions.push(ColumnDescription {
            column: field.name().clone(),
            dtype: format_dtype(field.data_type()),
            count: total_rows,
            nulls: null_count,
            null_pct: (null_pct * 100.0).round() / 100.0,
            min,
            max,
            mean: mean.map(|m| (m * 10000.0).round() / 10000.0),
            stddev: stddev.map(|s| (s * 10000.0).round() / 10000.0),
            distinct,
            top: if top.is_empty() { None } else { Some(top) },
        });
    }

    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    match format {
        Format::Json | Format::JsonLines => {
            let json = serde_json::to_value(&descriptions)?;
            crate::output::render_value(&mut writer, &json, format)?;
        }
        Format::Table => {
            use comfy_table::{
                modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Cell, ContentArrangement, Table,
            };
            let mut table = Table::new();
            table
                .load_preset(UTF8_FULL)
                .apply_modifier(UTF8_ROUND_CORNERS)
                .set_content_arrangement(ContentArrangement::Dynamic);

            table.set_header(vec![
                "Column", "Type", "Count", "Nulls", "Null%", "Min", "Max", "Mean", "Stddev",
                "Distinct",
            ]);

            for d in &descriptions {
                table.add_row(vec![
                    Cell::new(&d.column),
                    Cell::new(&d.dtype),
                    Cell::new(d.count),
                    Cell::new(d.nulls),
                    Cell::new(format!("{:.1}%", d.null_pct)),
                    Cell::new(
                        d.min
                            .as_ref()
                            .map(format_json_value)
                            .unwrap_or_else(|| "-".to_string()),
                    ),
                    Cell::new(
                        d.max
                            .as_ref()
                            .map(format_json_value)
                            .unwrap_or_else(|| "-".to_string()),
                    ),
                    Cell::new(
                        d.mean
                            .map(|v| format!("{v:.4}"))
                            .unwrap_or_else(|| "-".to_string()),
                    ),
                    Cell::new(
                        d.stddev
                            .map(|v| format!("{v:.4}"))
                            .unwrap_or_else(|| "-".to_string()),
                    ),
                    Cell::new(d.distinct),
                ]);
            }

            writeln!(writer, "{table}")?;
        }
        _ => {
            for d in &descriptions {
                writeln!(
                    writer,
                    "{}\t{}\t{}\t{}\t{:.1}%\t{}\t{}\t{}\t{}\t{}",
                    d.column,
                    d.dtype,
                    d.count,
                    d.nulls,
                    d.null_pct,
                    d.min.as_ref().map(format_json_value).unwrap_or_else(|| "-".to_string()),
                    d.max.as_ref().map(format_json_value).unwrap_or_else(|| "-".to_string()),
                    d.mean.map(|v| format!("{v:.4}")).unwrap_or_else(|| "-".to_string()),
                    d.stddev.map(|v| format!("{v:.4}")).unwrap_or_else(|| "-".to_string()),
                    d.distinct,
                )?;
            }
        }
    }

    Ok(())
}

fn format_dtype(dt: &DataType) -> String {
    match dt {
        DataType::Utf8 | DataType::LargeUtf8 => "string".to_string(),
        DataType::Int8 => "int8".to_string(),
        DataType::Int16 => "int16".to_string(),
        DataType::Int32 => "int32".to_string(),
        DataType::Int64 => "int64".to_string(),
        DataType::UInt8 => "uint8".to_string(),
        DataType::UInt16 => "uint16".to_string(),
        DataType::UInt32 => "uint32".to_string(),
        DataType::UInt64 => "uint64".to_string(),
        DataType::Float32 => "float32".to_string(),
        DataType::Float64 => "float64".to_string(),
        DataType::Boolean => "bool".to_string(),
        DataType::Date32 | DataType::Date64 => "date".to_string(),
        DataType::Timestamp(_, _) => "timestamp".to_string(),
        DataType::List(f) => format!("list<{}>", format_dtype(f.data_type())),
        DataType::Struct(fields) => {
            format!("struct<{} fields>", fields.len())
        }
        other => format!("{other:?}"),
    }
}

fn format_json_value(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "-".to_string(),
        other => other.to_string(),
    }
}

fn compute_numeric_stats(
    array: &dyn Array,
) -> (
    Option<serde_json::Value>,
    Option<serde_json::Value>,
    Option<f64>,
    Option<f64>,
) {
    macro_rules! numeric_stats {
        ($arr_type:ty, $array:expr) => {{
            let arr = $array.as_any().downcast_ref::<$arr_type>();
            match arr {
                Some(a) => {
                    let min = arrow::compute::min(a);
                    let max = arrow::compute::max(a);
                    let values: Vec<f64> = a
                        .iter()
                        .filter_map(|v| v.map(|x| x as f64))
                        .collect();
                    let (mean, stddev) = if values.is_empty() {
                        (None, None)
                    } else {
                        let sum: f64 = values.iter().sum();
                        let mean = sum / values.len() as f64;
                        let variance: f64 =
                            values.iter().map(|v| (v - mean).powi(2)).sum::<f64>()
                                / values.len() as f64;
                        (Some(mean), Some(variance.sqrt()))
                    };
                    let min_json = min.map(|v| serde_json::json!(v));
                    let max_json = max.map(|v| serde_json::json!(v));
                    (min_json, max_json, mean, stddev)
                }
                None => (None, None, None, None),
            }
        }};
    }

    match array.data_type() {
        DataType::Int8 => numeric_stats!(Int8Array, array),
        DataType::Int16 => numeric_stats!(Int16Array, array),
        DataType::Int32 => numeric_stats!(Int32Array, array),
        DataType::Int64 => numeric_stats!(Int64Array, array),
        DataType::UInt8 => numeric_stats!(UInt8Array, array),
        DataType::UInt16 => numeric_stats!(UInt16Array, array),
        DataType::UInt32 => numeric_stats!(UInt32Array, array),
        DataType::UInt64 => numeric_stats!(UInt64Array, array),
        DataType::Float32 => numeric_stats!(Float32Array, array),
        DataType::Float64 => numeric_stats!(Float64Array, array),
        DataType::Utf8 => {
            let arr = array.as_any().downcast_ref::<StringArray>().unwrap();
            let min = arrow::compute::min_string(arr).map(|s| serde_json::json!(s));
            let max = arrow::compute::max_string(arr).map(|s| serde_json::json!(s));
            (min, max, None, None)
        }
        DataType::Boolean => {
            let arr = array.as_any().downcast_ref::<BooleanArray>().unwrap();
            let min = arrow::compute::min_boolean(arr).map(|b| serde_json::json!(b));
            let max = arrow::compute::max_boolean(arr).map(|b| serde_json::json!(b));
            let values: Vec<f64> = arr.iter().filter_map(|v| v.map(|b| if b { 1.0 } else { 0.0 })).collect();
            let (mean, stddev) = if values.is_empty() {
                (None, None)
            } else {
                let sum: f64 = values.iter().sum();
                let mean = sum / values.len() as f64;
                let variance: f64 = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
                (Some(mean), Some(variance.sqrt()))
            };
            (min, max, mean, stddev)
        }
        _ => (None, None, None, None),
    }
}

fn compute_distinct_top_k(array: &dyn Array, top_k: usize) -> (usize, Vec<FreqEntry>) {
    use std::collections::HashMap;

    // Convert each element to a string representation for counting
    let formatter = arrow::util::display::ArrayFormatter::try_new(array, &Default::default());
    let formatter = match formatter {
        Ok(f) => f,
        Err(_) => return (0, Vec::new()),
    };

    let mut counts: HashMap<String, usize> = HashMap::new();
    for i in 0..array.len() {
        if array.is_null(i) {
            *counts.entry("null".to_string()).or_default() += 1;
        } else {
            let s = formatter.value(i).to_string();
            *counts.entry(s).or_default() += 1;
        }
    }

    let distinct = counts.len();

    let mut freq: Vec<(String, usize)> = counts.into_iter().collect();
    freq.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    freq.truncate(top_k);

    let top: Vec<FreqEntry> = freq
        .into_iter()
        .map(|(value, count)| FreqEntry {
            value: if value == "null" {
                serde_json::Value::Null
            } else {
                serde_json::Value::String(value)
            },
            count,
        })
        .collect();

    (distinct, top)
}
