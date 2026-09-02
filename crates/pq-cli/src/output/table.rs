use crate::commands::write_output::{column_indices, union_columns, CsvColumn};
use arrow::array::RecordBatch;
use arrow::util::display::ArrayFormatter;
use comfy_table::{
    modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Attribute, Cell, Color, ContentArrangement,
    Table,
};
use std::io::Write;

/// Shown for a column that a given row's *file* does not have at all --
/// never for a column the file has whose value happens to be SQL NULL.
///
/// A genuine NULL already renders as a blank cell (arrow's `ArrayFormatter`
/// with default `FormatOptions` prints nulls as the empty string, and this
/// predates this fix -- `pq cat nulls.parquet -f table` has always shown a
/// blank cell for a null, matching `-f csv`'s empty field). Reusing blank for
/// "this file has no such column" would make the two cases indistinguishable
/// in a multi-file table where the reader has no other way to tell "empty
/// value" from "value doesn't exist here". A one-character marker is cheap
/// and keeps that distinction visible without disturbing the established
/// blank-means-null convention.
const MISSING_COLUMN_MARKER: &str = "\u{b7}"; // ·

/// Build the header cells for `columns` -- the union of every batch's schema,
/// aligned by `(name, occurrence)` exactly as `-f csv` aligns it (see
/// `write_output::union_columns`). A duplicate name repeats in the header,
/// same as CSV's `id,id`, rather than being deduplicated away.
fn header_cells(columns: &[CsvColumn], color: bool) -> Vec<Cell> {
    columns
        .iter()
        .map(|c| {
            let cell = Cell::new(c.name());
            if color {
                cell.add_attribute(Attribute::Bold).fg(Color::Cyan)
            } else {
                cell
            }
        })
        .collect()
}

/// One batch's formatters, one per union column: `Some` when this batch's
/// schema has that column (resolved positionally, so a duplicate name
/// resolves to the matching occurrence, not just the first), `None` when it
/// doesn't.
fn batch_formatters<'a>(
    batch: &'a RecordBatch,
    columns: &[CsvColumn],
) -> Vec<Option<ArrayFormatter<'a>>> {
    let schema = batch.schema();
    column_indices(columns, schema.as_ref())
        .into_iter()
        .map(|index| {
            index.map(|i| {
                ArrayFormatter::try_new(batch.column(i).as_ref(), &Default::default()).unwrap()
            })
        })
        .collect()
}

pub fn render_table(writer: &mut dyn Write, batches: &[RecordBatch]) -> std::io::Result<()> {
    if batches.is_empty() {
        writeln!(writer, "(no data)")?;
        return Ok(());
    }

    // Union header, resolved by name (and by occurrence within a name) per
    // batch -- the same alignment `-f csv` uses. Freezing the header from
    // `batches[0]`'s schema, as this renderer used to, breaks the moment a
    // later file has the same column names in a different order: values are
    // then zipped in positionally under the first file's header, silently
    // swapping columns that merely happen to share names across files.
    let columns = union_columns(batches.iter().map(|b| b.schema()));

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic);

    // Header. Bold+cyan only when `--color` (and NO_COLOR / TTY detection,
    // for the `auto` default) resolved to "on" -- see
    // `output::configure_color`. comfy-table never emits ANSI codes of its
    // own accord; it only styles a cell if we explicitly attach a color or
    // attribute to it, so this `if` is the entire color feature.
    //
    // comfy-table *also* has its own independent tty check
    // (`Table::should_style`) that silently drops any attached styling when
    // real stdout isn't a terminal -- which would make `--color=always`
    // into a lie again when output is piped or captured (as in tests, or
    // under `PQ_FORCE_TTY`, which comfy-table doesn't know about). Force it
    // to respect *our* decision instead of its own.
    let color = super::color_enabled();
    if color {
        table.enforce_styling();
    }
    table.set_header(header_cells(&columns, color));

    // Rows
    for batch in batches {
        let formatters = batch_formatters(batch, &columns);

        for row_idx in 0..batch.num_rows() {
            let cells: Vec<Cell> = formatters
                .iter()
                .map(|f| match f {
                    Some(fmt) => Cell::new(fmt.value(row_idx).to_string()),
                    None => {
                        let cell = Cell::new(MISSING_COLUMN_MARKER);
                        if color {
                            cell.add_attribute(Attribute::Dim)
                        } else {
                            cell
                        }
                    }
                })
                .collect();
            table.add_row(cells);
        }
    }

    writeln!(writer, "{table}")?;
    Ok(())
}

pub fn render_plain(writer: &mut dyn Write, batches: &[RecordBatch]) -> std::io::Result<()> {
    if batches.is_empty() {
        return Ok(());
    }

    // Same by-name/by-occurrence alignment as `render_table` and `-f csv` --
    // see `render_table`'s comment. `-f plain` had the identical positional
    // bug (it also built its header from `batches[0]`'s schema alone).
    let columns = union_columns(batches.iter().map(|b| b.schema()));
    let headers: Vec<&str> = columns.iter().map(|c| c.name()).collect();
    writeln!(writer, "{}", headers.join("\t"))?;

    for batch in batches {
        let formatters = batch_formatters(batch, &columns);

        for row_idx in 0..batch.num_rows() {
            let cells: Vec<String> = formatters
                .iter()
                .map(|f| match f {
                    Some(fmt) => fmt.value(row_idx).to_string(),
                    None => MISSING_COLUMN_MARKER.to_string(),
                })
                .collect();
            writeln!(writer, "{}", cells.join("\t"))?;
        }
    }
    Ok(())
}
