use arrow::array::RecordBatch;
use std::io::Write;

pub fn render_json(writer: &mut dyn Write, batches: &[RecordBatch]) -> std::io::Result<()> {
    let rows: Vec<serde_json::Value> = batches
        .iter()
        .flat_map(pq_query::convert::batch_to_json_rows)
        .collect();
    serde_json::to_writer_pretty(&mut *writer, &rows)?;
    writeln!(writer)?;
    Ok(())
}

pub fn render_jsonl(writer: &mut dyn Write, batches: &[RecordBatch]) -> std::io::Result<()> {
    for batch in batches {
        for row in pq_query::convert::batch_to_json_rows(batch) {
            serde_json::to_writer(&mut *writer, &row)?;
            writeln!(writer)?;
        }
    }
    Ok(())
}
