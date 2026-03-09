use std::path::Path;

pub fn run(file: &str, columns: &[String], output: &str) -> anyhow::Result<()> {
    let path = Path::new(file);
    let opts = pq_transform::select::SelectOptions {
        columns: columns.to_vec(),
        output: output.to_string(),
        compression: parquet::basic::Compression::ZSTD(Default::default()),
    };

    let rows = pq_transform::select::select_columns(path, &opts)?;
    eprintln!("Wrote {rows} rows to {output}");
    Ok(())
}
