use std::path::Path;

use crate::cli::InputFormatArg;

pub fn run(input: &str, output: &str, input_format: Option<&InputFormatArg>) -> anyhow::Result<()> {
    let path = Path::new(input);

    let fmt = match input_format {
        Some(InputFormatArg::Json) => pq_transform::convert::InputFormat::Json,
        Some(InputFormatArg::Jsonl) => pq_transform::convert::InputFormat::JsonLines,
        Some(InputFormatArg::Csv) => pq_transform::convert::InputFormat::Csv,
        None => {
            // Auto-detect from extension
            match path.extension().and_then(|e| e.to_str()) {
                Some("json") => pq_transform::convert::InputFormat::Json,
                Some("jsonl" | "ndjson") => pq_transform::convert::InputFormat::JsonLines,
                Some("csv") => pq_transform::convert::InputFormat::Csv,
                _ => pq_transform::convert::InputFormat::JsonLines,
            }
        }
    };

    let opts = pq_transform::convert::ConvertOptions {
        input_format: fmt,
        output: output.to_string(),
        compression: parquet::basic::Compression::ZSTD(Default::default()),
    };

    let rows = pq_transform::convert::convert_json_to_parquet(path, &opts)?;
    super::write_output::print_status(output, &format!("Converted {rows} rows to {output}"));
    Ok(())
}
