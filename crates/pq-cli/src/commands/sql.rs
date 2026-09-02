use crate::output::{self, Format};

pub fn run(query: &str, output: Option<&str>, format: Format) -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    let batches = rt.block_on(pq_query::sql::execute_sql(query))?;

    match output {
        Some(path) => {
            // DataFusion has already collected every batch into memory above,
            // so `-o` naming an input file was not destructive here. Staged
            // anyway for atomicity: a write that fails half way must not leave
            // a truncated file where the user's data was.
            let rows = pq_transform::output_guard::with_atomic_output(
                path,
                |staged| -> anyhow::Result<usize> {
                    super::write_output::write_batches_to_file(
                        staged.to_str().unwrap_or(path),
                        &batches,
                    )
                },
            )?;
            eprintln!("Wrote {rows} rows to {path}");
        }
        None => {
            let stdout = std::io::stdout();
            let mut writer = stdout.lock();
            output::render_batches(&mut writer, &batches, format)?;
        }
    }
    Ok(())
}
