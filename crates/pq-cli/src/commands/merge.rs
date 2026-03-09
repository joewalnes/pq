use std::path::Path;

use crate::cli::SchemaModeArg;

pub fn run(files: &[String], output: &str, schema_mode: &SchemaModeArg) -> anyhow::Result<()> {
    let paths: Vec<&Path> = files.iter().map(|f| Path::new(f.as_str())).collect();
    let mode = match schema_mode {
        SchemaModeArg::Strict => pq_transform::merge::SchemaMode::Strict,
        SchemaModeArg::Union => pq_transform::merge::SchemaMode::Union,
        SchemaModeArg::Intersect => pq_transform::merge::SchemaMode::Intersect,
    };

    let opts = pq_transform::merge::MergeOptions {
        schema_mode: mode,
        output: output.to_string(),
        compression: parquet::basic::Compression::ZSTD(Default::default()),
    };

    let rows = pq_transform::merge::merge_files(&paths, &opts)?;
    eprintln!(
        "Merged {} files, wrote {rows} rows to {output}",
        files.len()
    );
    Ok(())
}
