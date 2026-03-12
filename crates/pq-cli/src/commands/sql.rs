use crate::output::{self, Format};

pub fn run(query: &str, output: Option<&str>, format: Format) -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    let batches = rt.block_on(pq_query::sql::execute_sql(query))?;

    match output {
        Some(path) => {
            let rows = super::write_output::write_batches_to_file(path, &batches)?;
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
