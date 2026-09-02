use std::collections::HashMap;
use std::io::Write;

use arrow::array::*;
use arrow::datatypes::*;

use crate::output::Format;

// `reorder_batch_to_schema` below resolves duplicate-named columns by
// (name, occurrence) rather than by name alone. This is the same identity
// unit `write_output::union_columns`/`column_indices` already use to align
// CSV/table output across files with duplicate column names — reused here
// rather than reinvented, per the project's own lesson that two answers to
// one question ("what does column identity mean when a name repeats?") is
// how bugs get born.
use super::write_output::{column_indices, union_columns};

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
    // The guard's job is narrower than "these two `Schema`s are equal", and
    // it was wrong in *both* directions at different times:
    //
    //   - Comparing whole `arrow::datatypes::Field`s with `!=` over-rejected:
    //     `Field::eq` also compares `nullable` and per-field `metadata`
    //     (arrow-schema-53.4.1/src/field.rs:52-58), which
    //     `arrow::compute::concat` (arrow-select-53.4.1/src/concat.rs:150-161)
    //     never looks at — it operates on bare `&dyn Array` values, which
    //     don't carry field names or metadata at all, and only checks
    //     `array.data_type() != d`. That rejected files `concat` handles
    //     fine: a NOT NULL column next to a nullable one, or files from
    //     different writers that set field metadata differently.
    //
    //   - The fix for that (comparing only `DataType`, dropping `name` from
    //     the comparison along with `nullable`/`metadata`) *under*-rejected:
    //     since `concat` itself never sees names, two files with completely
    //     disjoint column names but matching types passed the guard, and
    //     were then concatenated by column *position* and labelled with the
    //     first file's names. `omega`'s values were silently reported under
    //     the name `alpha`, exit 0, no note — a silent wrong answer, not
    //     merely an over-strict refusal. Column *names* are pq's own
    //     invariant for what "combine by position" is allowed to mean, not
    //     `concat`'s: `concat` would happily paste unrelated columns
    //     together, but `describe`'s output is only meaningful if position
    //     N is the same logical column across every file.
    //
    // `schemas_concat_compatible` below checks what both requirements
    // demand: the same set of (name, `DataType`) pairs, comparing names
    // exactly (case-sensitive) — order-independent, because two files
    // holding the same columns in a different order (e.g. `amount, price`
    // vs `price, amount` — writers that don't agree on column order) are
    // common and safe to combine. Order independence is paired with
    // `reorder_batch_to_schema` below, which physically permutes a later
    // file's columns to the first file's order before `concat` ever sees
    // them, so a name-set match here is only ever used once the underlying
    // data has actually been realigned to match.
    let mut file_meta: Vec<(&str, arrow::datatypes::SchemaRef, i64)> = Vec::new();
    for f in files {
        let (schema, rows) = pq_core::reader::open_metadata(f)?;
        if let Some((first_file, first_schema, _)) = file_meta.first() {
            if !schemas_concat_compatible(first_schema, &schema) {
                anyhow::bail!(
                    "Cannot describe files with different schemas: \
                     '{first_file}' has columns [{}], but '{f}' has columns [{}]. \
                     `stats --describe` combines rows across files by column \
                     identity, so every file must have the same column names \
                     and types (column order, nullability, and field metadata \
                     may differ).",
                    describe_columns(first_schema),
                    describe_columns(&schema),
                );
            }
        }
        file_meta.push((f.as_str(), schema, rows));
    }
    let total_rows_meta: i64 = file_meta.iter().map(|(_, _, rows)| rows).sum();
    let canonical_schema = file_meta[0].1.clone();

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
        // `schemas_concat_compatible` above accepts a later file whose
        // columns are the same set as the first file's but in a different
        // order. That check alone would be unsound without this: the
        // column-by-column loop after this function names every position by
        // the *first* file's schema (`all_batches[0].schema()`), so a batch
        // whose physical column order doesn't match the first file's would
        // still get concatenated position-by-position and mislabelled —
        // exactly the silent-swap failure mode this fix exists to remove,
        // just moved from "different names" to "same names, different
        // order". Realigning here, once, keeps that loop's position-based
        // indexing valid for every batch it touches.
        for batch in batches {
            all_batches.push(reorder_batch_to_schema(batch, &canonical_schema)?);
        }
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

/// True when `describe` may combine these two schemas' columns: the same
/// multiset of (name, `DataType`) pairs, regardless of column order. Name
/// comparison is exact and case-sensitive — `Alpha` and `alpha` are
/// different columns. Nullability and per-field metadata are deliberately
/// excluded, matching what `arrow::compute::concat` itself requires (see the
/// comment at its call site above); order is deliberately tolerated because
/// the caller (`run`, above) physically realigns later files' columns to the
/// first file's order via `reorder_batch_to_schema` before concatenation, so
/// a name-set match here is never used against misaligned data.
///
/// This is deliberately narrower than `Schema`/`Field` equality in the
/// nullability/metadata direction, and deliberately *stricter* than a bare
/// `DataType`-only comparison in the name direction: dropping names from
/// this check (as a prior version did) let two files with entirely
/// disjoint column names — both happening to share a `DataType` — pass
/// silently, and `run`'s position-based indexing then mislabelled the
/// second file's data under the first file's column names.
fn schemas_concat_compatible(a: &Schema, b: &Schema) -> bool {
    field_multiset(a) == field_multiset(b)
}

/// The sorted (name, `DataType`) pairs of a schema's fields, order-erased so
/// two schemas that agree on columns but not their sequence compare equal.
fn field_multiset(schema: &Schema) -> Vec<(&str, &DataType)> {
    let mut pairs: Vec<(&str, &DataType)> = schema
        .fields()
        .iter()
        .map(|f| (f.name().as_str(), f.data_type()))
        .collect();
    pairs.sort();
    pairs
}

/// Permute `batch`'s columns into `canonical`'s field order by (name,
/// occurrence). A no-op (returns `batch` unchanged) when the batch's fields
/// are already in that order, which covers the overwhelmingly common case
/// (every file agrees on order) without paying for a projection.
///
/// **Why (name, occurrence) and not name alone.** The previous
/// implementation resolved each canonical field with `Schema::index_of`,
/// which returns the *first* field of a given name. Parquet legally permits
/// duplicate column names, so with a repeated name every occurrence beyond
/// the first resolved to that same first index: the projection then
/// selected one physical column multiple times and never selected at least
/// one other, which `concat` cannot detect (it received a same-shaped,
/// same-typed input) and silently produced a wrong count/min/max/mean built
/// from the wrong data — not a refusal, not an error, just a wrong number.
/// Reproduction: `d1` = `a,a,x` = `[1,2,3],[100,200,300],[7,8,9]`, `d2` =
/// `x,a,a` = `[11,12,13],[1100,1200,1300],[70,80,90]` (same multiset,
/// different physical order). `index_of("a")` on `d2` always returns the
/// position of `d2`'s *first* `a` (`[1100,1200,1300]`), so the second `a`
/// column's own stats were computed from `[100,200,300]` doubled up with
/// `[1100,1200,1300]` while `[70,80,90]` never appeared anywhere in the
/// output. pyarrow ground truth for that second `a` is min 70 / max 300 /
/// mean 140.0.
///
/// Keying on (name, occurrence) — the same identity unit
/// `write_output::union_columns`/`column_indices` already use for CSV/table
/// output — restores positional identity: the *k*-th field named `a` in
/// `canonical` is matched to the *k*-th field named `a` in `batch`,
/// regardless of what other columns sit between occurrences in either
/// file's own layout. This is a deliberate, documented convention, not a
/// verified fact: Parquet carries no information that distinguishes same-
/// named, same-typed columns from each other, so "first `a` corresponds to
/// first `a`" is a choice, not a certainty. It is the same choice CSV/table
/// output already makes for the identical question, and refusing here while
/// answering it there would be a second, disagreeing answer to one
/// question. `[a,a,x]` vs `[x,a,a]` and `[a,a,x]` vs `[a,x,a]` are both
/// resolved by this rule with equal confidence: "occurrence" is each file's
/// own encounter order among same-named fields, which is unaffected by
/// which other columns are interposed, so there is no meaningfully
/// "more ambiguous" case among same-multiset permutations for this
/// function to refuse.
///
/// What this function *does* refuse: a (name, occurrence) pairing that
/// disagrees on `DataType`. `schemas_concat_compatible`'s multiset check
/// compares *sorted* (name, `DataType`) pairs, so it cannot catch a file
/// where a repeated name's own types are merely permuted among its
/// occurrences (e.g. file A has `a:Int64` then `a:Utf8`; file B has
/// `a:Utf8` then `a:Int64` — both sort to the same multiset). Matching by
/// occurrence order would then pair `Int64` with `Utf8`. `concat` would
/// eventually catch that (it refuses to concatenate arrays of different
/// `DataType`s) but with a raw Arrow error, once record-batch-internal
/// positions have already been shuffled — checked explicitly and up front
/// here instead, so the failure names the actual duplicate column and
/// occurrence rather than surfacing however far downstream `concat` happens
/// to notice.
///
/// Only ever called after `schemas_concat_compatible` has confirmed the
/// batch's schema has exactly the same (name, `DataType`) *multiset* as
/// `canonical`, so every canonical (name, occurrence) pair is expected to
/// have a same-named match in `batch`; a missing match here indicates that
/// invariant was violated by the caller, not a normal data condition, so it
/// is propagated as an error rather than panicking.
fn reorder_batch_to_schema(
    batch: RecordBatch,
    canonical: &SchemaRef,
) -> anyhow::Result<RecordBatch> {
    let batch_schema = batch.schema();
    // Names AND types must line up position-by-position for this to be a
    // true no-op. Name alone is not enough: a duplicate name's own
    // occurrences can have their types permuted across files in a way that
    // still lines up name-for-name by position (see the type-mismatch
    // paragraph above) while disagreeing on type — checking name only here
    // would wave that batch through unprojected and let the type mismatch
    // surface later as a raw `concat` error instead of the clear one below.
    let already_in_order = batch_schema
        .fields()
        .iter()
        .zip(canonical.fields())
        .all(|(a, b)| a.name() == b.name() && a.data_type() == b.data_type());
    if already_in_order {
        return Ok(batch);
    }

    let columns = union_columns(std::iter::once(canonical.clone()));
    let batch_indices = column_indices(&columns, &batch_schema);

    let mut occurrence_of: HashMap<&str, usize> = HashMap::new();
    let mut projection = Vec::with_capacity(columns.len());
    for (canonical_idx, (column, batch_idx)) in columns.iter().zip(&batch_indices).enumerate() {
        let occurrence = occurrence_of.entry(column.name()).or_insert(0);
        let this_occurrence = *occurrence;
        *occurrence += 1;

        let batch_idx = batch_idx.ok_or_else(|| {
            anyhow::anyhow!(
                "internal error: column '{}' (occurrence {this_occurrence}) not \
                 found while realigning a batch already confirmed schema-compatible",
                column.name(),
            )
        })?;

        let canonical_field = canonical.field(canonical_idx);
        let batch_field = batch_schema.field(batch_idx);
        if batch_field.data_type() != canonical_field.data_type() {
            let name = column.name();
            let canonical_type = canonical_field.data_type();
            let batch_type = batch_field.data_type();
            anyhow::bail!(
                "Cannot describe files with different schemas: column \
                 '{name}' (occurrence {this_occurrence} of that name) has \
                 type {canonical_type:?} in one file's column order and \
                 {batch_type:?} in another's. `stats --describe` matches \
                 duplicate-named columns across files by order of \
                 occurrence (1st '{name}' matches 1st '{name}', 2nd matches \
                 2nd, ...) once every other check has passed; this pairing \
                 disagrees on type, so combining it would require guessing \
                 which physical column is 'the same' — refusing instead of \
                 producing a number built from mismatched data.",
            );
        }

        projection.push(batch_idx);
    }

    Ok(batch.project(&projection)?)
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
