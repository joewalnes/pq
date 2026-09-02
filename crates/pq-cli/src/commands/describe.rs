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

/// Per-file accounting for the `--sample-size` row budget: how many rows a
/// named file actually contributed, versus how many it holds. A file with
/// `rows_read == 0` was schema-checked (see the metadata pass in `run`
/// below) but never opened for data — the budget was exhausted before the
/// reader reached it.
#[derive(Debug, serde::Serialize)]
pub struct FileSampling {
    pub path: String,
    pub rows_total: i64,
    pub rows_read: usize,
    /// False only when the row budget was exhausted before the reader ever
    /// reached this file, so it contributed nothing to the sample. This is
    /// distinct from `rows_read == 0`: a genuinely empty file that *was*
    /// opened (its schema checked, its zero rows read) has `opened: true`,
    /// `rows_read: 0` — it must not be reported as "not read" alongside a
    /// file the budget skipped outright.
    pub opened: bool,
}

/// Discloses the sampling fact to every output format, `json`/`jsonl`
/// included. Before this existed, `table`/`plain` printed a "sampled" note
/// but `json`/`jsonl` carried nothing at all — a machine consumer had no way
/// to tell it had received a partial answer, let alone which of the named
/// files were actually read.
#[derive(Debug, serde::Serialize)]
pub struct SamplingInfo {
    /// True when fewer rows were read than exist across all named files.
    pub sampled: bool,
    /// The `--sample-size` value as given; 0 means unlimited (no cap).
    pub sample_size: usize,
    pub rows_read: usize,
    pub rows_total: usize,
    pub files_total: usize,
    pub files_read: usize,
    pub files: Vec<FileSampling>,
}

#[derive(Debug, serde::Serialize)]
pub struct DescribeReport {
    pub sampling: SamplingInfo,
    pub columns: Vec<ColumnDescription>,
}

pub fn run(
    files: &[String],
    top_k: usize,
    sample_size: usize,
    format: Format,
) -> anyhow::Result<()> {
    if files.is_empty() {
        anyhow::bail!("No files given");
    }

    // Pass 1: metadata only (schema + row count), for EVERY named file,
    // before any row budget is applied.
    //
    // This exists because `--sample-size` used to gate which files were
    // even *opened*: the old single loop read data with a shrinking `limit`
    // and `break`-ed the moment the budget hit zero, so a file the budget
    // never reached was never opened at all — not for data, and not for its
    // schema. Two files holding [1,2,3] and [100,200,300], `--sample-size
    // 2`, reported `count: 2, max: 2` drawn entirely from the first file,
    // exit 0, no indication the second file was ignored. Worse, a second
    // file with an outright incompatible schema (e.g. a string column where
    // the first file has an int) passed through unnoticed the same way,
    // because the schema-compatibility guard below never got to see it.
    //
    // Reading metadata for every file up front (cheap: footer only, no row
    // data) fixes both: the guard now fires for every named file regardless
    // of the sample size, and the true total row count is known before
    // deciding how the budget gets spent in pass 2.
    //
    // The guard must reject exactly what the column-by-column
    // `arrow::compute::concat` call below would reject, no more. An earlier
    // version compared whole `arrow::datatypes::Field`s with `!=`, but
    // `Field`'s `PartialEq` also compares `nullable` and per-field
    // `metadata` (arrow-schema-53.4.1/src/field.rs:52-59) — properties
    // `concat` never looks at (arrow-select-53.4.1/src/concat.rs:160-165
    // compares only `data_type()`). That over-rejected files that `concat`
    // handles fine: e.g. a file written with a NOT NULL column next to one
    // without, or files from different writers that set field metadata
    // differently. `schemas_concat_compatible` below checks only what
    // `concat` actually requires: same column count, same `DataType` in
    // each position.
    let mut file_meta: Vec<(&str, arrow::datatypes::SchemaRef, i64)> = Vec::new();
    for f in files {
        let (schema, rows) = pq_core::reader::open_metadata(f)?;
        if let Some((first_file, first_schema, _)) = file_meta.first() {
            if !schemas_concat_compatible(first_schema, &schema) {
                anyhow::bail!(
                    "Cannot describe files with different schemas: \
                     '{first_file}' has columns [{}], but '{f}' has columns [{}]. \
                     `stats --describe` combines rows across files by column \
                     position, so every file must have the same number of \
                     columns with matching types (nullability and field \
                     metadata may differ).",
                    describe_columns(first_schema),
                    describe_columns(&schema),
                );
            }
        }
        file_meta.push((f.as_str(), schema, rows));
    }
    let total_rows_meta: i64 = file_meta.iter().map(|(_, _, rows)| rows).sum();

    let effective_limit = if sample_size > 0 {
        Some(sample_size)
    } else {
        None
    };

    // Pass 2: read up to the row budget, in file order. Multiple files are
    // treated as one logical concatenation and `--sample-size` as a total
    // row budget across it — the same rule already chosen for `tail`/
    // `sample` (see DIARY.md, "Multi-file semantics for `tail`/`sample`:
    // concatenation, not per-file"): a per-file cap would silently multiply
    // the amount of data read by the file count for an unchanged flag.
    //
    // A file the budget never reaches contributes 0 rows here — that part
    // of the old behaviour is unchanged and is the correct reading of "the
    // first N rows of the concatenation" — but unlike before, it was still
    // schema-checked above, and it is still named in `sampling.files` below
    // with `rows_read: 0`. Silently vanishing from the report is what the
    // old code did; being visibly and honestly excluded is what this does
    // instead.
    let mut all_batches: Vec<RecordBatch> = Vec::new();
    let mut rows_remaining = effective_limit;
    let mut file_reads: Vec<FileSampling> = Vec::new();
    for (f, _schema, rows_total) in &file_meta {
        if rows_remaining == Some(0) {
            file_reads.push(FileSampling {
                path: (*f).to_string(),
                rows_total: *rows_total,
                rows_read: 0,
                opened: false,
            });
            continue;
        }
        let file_opts = pq_core::reader::ReadOptions {
            limit: rows_remaining,
            ..Default::default()
        };
        let (batches, _schema) = pq_core::reader::open_batches(f, &file_opts)?;
        let batch_rows: usize = batches.iter().map(|b| b.num_rows()).sum();
        file_reads.push(FileSampling {
            path: (*f).to_string(),
            rows_total: *rows_total,
            rows_read: batch_rows,
            opened: true,
        });
        all_batches.extend(batches);
        if let Some(ref mut rem) = rows_remaining {
            *rem = rem.saturating_sub(batch_rows);
        }
    }

    if all_batches.is_empty() {
        anyhow::bail!("No data found");
    }

    let schema = all_batches[0].schema();
    let sampled_rows: usize = all_batches.iter().map(|b| b.num_rows()).sum();
    let total_rows = total_rows_meta as usize;
    let is_sampled = effective_limit.is_some() && sampled_rows < total_rows;
    let files_read = file_reads.iter().filter(|fr| fr.opened).count();
    let unread_files: Vec<String> = file_reads
        .iter()
        .filter(|fr| !fr.opened)
        .map(|fr| fr.path.clone())
        .collect();
    let sampling = SamplingInfo {
        sampled: is_sampled,
        sample_size,
        rows_read: sampled_rows,
        rows_total: total_rows,
        files_total: file_reads.len(),
        files_read,
        files: file_reads,
    };

    // Concatenate all arrays per column
    let mut descriptions: Vec<ColumnDescription> = Vec::new();

    for (col_idx, field) in schema.fields().iter().enumerate() {
        let arrays: Vec<&dyn Array> = all_batches
            .iter()
            .map(|b| b.column(col_idx).as_ref())
            .collect();
        let concatenated = arrow::compute::concat(&arrays)?;

        let null_count = concatenated.null_count();
        let null_pct = if sampled_rows > 0 {
            (null_count as f64 / sampled_rows as f64) * 100.0
        } else {
            0.0
        };

        let (min, max, mean, stddev) = compute_numeric_stats(concatenated.as_ref());
        let (distinct, top) = compute_distinct_top_k(concatenated.as_ref(), top_k);

        descriptions.push(ColumnDescription {
            column: field.name().clone(),
            dtype: format_dtype(field.data_type()),
            count: sampled_rows,
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

    // Build this once so the "not read" note below (table/plain) and the
    // JSON/JSONL sampling field say exactly the same thing.
    let unread_note = if unread_files.is_empty() {
        String::new()
    } else {
        format!(" ({} not read)", unread_files.join(", "))
    };

    match format {
        Format::Json | Format::JsonLines => {
            // Every output format must carry the sampling fact, not just
            // `table`/`plain`'s printed note — a machine consumer of
            // `json`/`jsonl` used to receive a bare array with no way to
            // tell it held a partial answer, let alone which of the named
            // files it came from.
            let report = DescribeReport {
                sampling,
                columns: descriptions,
            };
            let json = serde_json::to_value(&report)?;
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
            if is_sampled {
                writeln!(
                    writer,
                    "Statistics computed from first {sampled_rows} of {total_rows} rows{unread_note} (use --sample-size 0 for all)"
                )?;
            }
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
                    d.min
                        .as_ref()
                        .map(format_json_value)
                        .unwrap_or_else(|| "-".to_string()),
                    d.max
                        .as_ref()
                        .map(format_json_value)
                        .unwrap_or_else(|| "-".to_string()),
                    d.mean
                        .map(|v| format!("{v:.4}"))
                        .unwrap_or_else(|| "-".to_string()),
                    d.stddev
                        .map(|v| format!("{v:.4}"))
                        .unwrap_or_else(|| "-".to_string()),
                    d.distinct,
                )?;
            }
            if is_sampled {
                writeln!(
                    writer,
                    "# Statistics computed from first {sampled_rows} of {total_rows} rows{unread_note}"
                )?;
            }
        }
    }

    Ok(())
}

/// True when `arrow::compute::concat` can combine these two schemas'
/// columns position-by-position: same number of fields, and each pair
/// sharing a `DataType`. This is deliberately narrower than `Schema`/`Field`
/// equality — nullability and field metadata are irrelevant to `concat`
/// (see the comment at its call site above) and must not cause a rejection
/// here.
fn schemas_concat_compatible(a: &Schema, b: &Schema) -> bool {
    a.fields().len() == b.fields().len()
        && a.fields()
            .iter()
            .zip(b.fields())
            .all(|(fa, fb)| fa.data_type() == fb.data_type())
}

/// Render a schema's fields as "name:type, name:type, ..." for the
/// different-schemas error.
///
/// Deliberately uses `DataType`'s `Debug` output rather than `format_dtype`
/// (the friendly renderer used for stats display): `format_dtype` collapses
/// distinct types onto the same string — every `Timestamp(_, _)` prints as
/// "timestamp" regardless of unit or timezone, every `Struct` with the same
/// field count prints as "struct<N fields>" regardless of what those fields
/// are. This function only runs once `schemas_concat_compatible` has found
/// a genuine `DataType` difference somewhere, and `DataType`'s `Debug` and
/// `PartialEq` are both derived over the same fields, so two schemas that
/// differ here are guaranteed to render differently — the error can never
/// again show two identical-looking column lists for a real mismatch.
fn describe_columns(schema: &Schema) -> String {
    schema
        .fields()
        .iter()
        .map(|f| format!("{}:{:?}", f.name(), f.data_type()))
        .collect::<Vec<_>>()
        .join(", ")
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
                    let values: Vec<f64> = a.iter().filter_map(|v| v.map(|x| x as f64)).collect();
                    let (mean, stddev) = if values.is_empty() {
                        (None, None)
                    } else {
                        let sum: f64 = values.iter().sum();
                        let mean = sum / values.len() as f64;
                        let variance: f64 = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>()
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
            let values: Vec<f64> = arr
                .iter()
                .filter_map(|v| v.map(|b| if b { 1.0 } else { 0.0 }))
                .collect();
            let (mean, stddev) = if values.is_empty() {
                (None, None)
            } else {
                let sum: f64 = values.iter().sum();
                let mean = sum / values.len() as f64;
                let variance: f64 =
                    values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
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
