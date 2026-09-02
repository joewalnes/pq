use arrow::array::RecordBatch;
use arrow::util::display::ArrayFormatter;
use comfy_table::{
    modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Attribute, Cell, Color, ContentArrangement,
    Table,
};
use std::io::Write;

pub fn render_table(writer: &mut dyn Write, batches: &[RecordBatch]) -> std::io::Result<()> {
    if batches.is_empty() {
        writeln!(writer, "(no data)")?;
        return Ok(());
    }

    let schema = batches[0].schema();
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
    let headers: Vec<Cell> = schema
        .fields()
        .iter()
        .map(|f| {
            let cell = Cell::new(f.name());
            if color {
                cell.add_attribute(Attribute::Bold).fg(Color::Cyan)
            } else {
                cell
            }
        })
        .collect();
    table.set_header(headers);

    // Rows
    for batch in batches {
        let formatters: Vec<ArrayFormatter> = (0..batch.num_columns())
            .map(|i| {
                ArrayFormatter::try_new(batch.column(i).as_ref(), &Default::default()).unwrap()
            })
            .collect();

        for row_idx in 0..batch.num_rows() {
            let cells: Vec<Cell> = formatters
                .iter()
                .map(|f| Cell::new(f.value(row_idx).to_string()))
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

    let schema = batches[0].schema();
    let headers: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();
    writeln!(writer, "{}", headers.join("\t"))?;

    for batch in batches {
        let formatters: Vec<ArrayFormatter> = (0..batch.num_columns())
            .map(|i| {
                ArrayFormatter::try_new(batch.column(i).as_ref(), &Default::default()).unwrap()
            })
            .collect();

        for row_idx in 0..batch.num_rows() {
            let cells: Vec<String> = formatters
                .iter()
                .map(|f| f.value(row_idx).to_string())
                .collect();
            writeln!(writer, "{}", cells.join("\t"))?;
        }
    }
    Ok(())
}
