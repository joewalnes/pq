use crate::output::{self, Format};

pub fn run(query: &str, format: Format) -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    let batches = rt.block_on(pq_query::sql::execute_sql(query))?;

    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    output::render_batches(&mut writer, &batches, format)?;
    Ok(())
}
