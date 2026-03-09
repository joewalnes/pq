use arrow::array::RecordBatch;
use arrow::util::display::ArrayFormatter;
use comfy_table::{
    modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Cell, ContentArrangement, Table,
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

    // Header
    let headers: Vec<Cell> = schema
        .fields()
        .iter()
        .map(|f| Cell::new(f.name()))
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
