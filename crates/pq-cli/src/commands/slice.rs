use std::path::Path;

pub fn run(file: &str, offset: usize, limit: usize, output: &str) -> anyhow::Result<()> {
    let path = Path::new(file);
    let opts = pq_transform::slice::SliceOptions {
        offset,
        limit,
        output: output.to_string(),
        compression: parquet::basic::Compression::ZSTD(Default::default()),
    };

    let rows = pq_transform::slice::slice_rows(path, &opts)?;
    eprintln!("Wrote {rows} rows to {output}");
    Ok(())
}
