use arrow::array::RecordBatch;
use arrow::util::display::ArrayFormatter;
use std::collections::HashSet;
use std::io::Write;

fn to_io_error(err: csv::Error) -> std::io::Error {
    std::io::Error::other(err)
}

pub fn render_csv(writer: &mut dyn Write, batches: &[RecordBatch]) -> std::io::Result<()> {
    if batches.is_empty() {
        return Ok(());
    }

    // Header is the union of every batch's schema field names, in
    // first-seen order — not just batch 0's. `pq cat a.parquet b.parquet -f
    // csv` can combine files with different schemas; freezing the header
    // from batch 0 either misaligns a later batch's values under the wrong
    // column or silently drops a column batch 0 didn't have. `batches` is
    // already fully resident (this function receives the whole slice), so
    // the union costs one extra pass over already-known field lists, not
    // extra buffering.
    let mut header: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for batch in batches {
        for field in batch.schema().fields() {
            if seen.insert(field.name().clone()) {
                header.push(field.name().clone());
            }
        }
    }

    // Uses the `csv` crate rather than hand-rolled quoting: a bare `\r` is
    // just as much a record separator to a compliant CSV reader as `\n` or
    // `\r\n`, but a hand-rolled check that only tests for `,`, `"`, and
    // `\n` leaves a lone `\r` unquoted, silently splitting one row into two
    // on read.
    let mut wtr = csv::WriterBuilder::new().from_writer(writer);
    wtr.write_record(&header).map_err(to_io_error)?;

    for batch in batches {
        let schema = batch.schema();
        // Map each header column to this batch's column index, if this
        // batch's schema has it — batches from a different file may not.
        let formatters: Vec<Option<ArrayFormatter>> = header
            .iter()
            .map(|name| {
                schema.index_of(name).ok().map(|i| {
                    ArrayFormatter::try_new(batch.column(i).as_ref(), &Default::default())
                        .expect("array formatter")
                })
            })
            .collect();

        for row_idx in 0..batch.num_rows() {
            let cells: Vec<String> = formatters
                .iter()
                .map(|f| match f {
                    Some(fmt) => fmt.value(row_idx).to_string(),
                    None => String::new(),
                })
                .collect();
            wtr.write_record(&cells).map_err(to_io_error)?;
        }
    }
    wtr.flush()
}
