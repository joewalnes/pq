use arrow::array::RecordBatch;
use arrow::util::display::ArrayFormatter;
use std::io::Write;

pub fn render_csv(writer: &mut dyn Write, batches: &[RecordBatch]) -> std::io::Result<()> {
    if batches.is_empty() {
        return Ok(());
    }

    let schema = batches[0].schema();

    // Header
    let header: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();
    writeln!(writer, "{}", header.join(","))?;

    // Rows
    for batch in batches {
        let formatters: Vec<ArrayFormatter> = (0..batch.num_columns())
            .map(|i| {
                ArrayFormatter::try_new(batch.column(i).as_ref(), &Default::default()).unwrap()
            })
            .collect();

        for row_idx in 0..batch.num_rows() {
            let cells: Vec<String> = formatters
                .iter()
                .map(|f| {
                    let val = f.value(row_idx).to_string();
                    // Quote if contains comma, newline, or quote
                    if val.contains(',') || val.contains('\n') || val.contains('"') {
                        format!("\"{}\"", val.replace('"', "\"\""))
                    } else {
                        val
                    }
                })
                .collect();
            writeln!(writer, "{}", cells.join(","))?;
        }
    }
    Ok(())
}
