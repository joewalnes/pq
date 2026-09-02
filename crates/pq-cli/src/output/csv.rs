use arrow::array::RecordBatch;
use std::io::Write;

/// Render batches as CSV to stdout.
///
/// Delegates to the single batch-to-CSV implementation in
/// `commands::write_output` rather than keeping a second one here. There
/// used to be four hand-rolled implementations of this — stdout, `cat
/// --output`, `export`, `sql -o` — and they disagreed: given a Parquet file
/// with two columns both named `id`, this path emitted the first column's
/// values and the file paths emitted the second's, each under a single `id`
/// header. Both had silently dropped a column; they did not even drop the
/// same one. One implementation is the fix for that class, not four
/// coincidentally-matching ones.
pub fn render_csv(writer: &mut dyn Write, batches: &[RecordBatch]) -> std::io::Result<()> {
    if batches.is_empty() {
        return Ok(());
    }
    crate::commands::write_output::write_batches_csv(writer, batches)
        .map(|_| ())
        .map_err(std::io::Error::other)
}
