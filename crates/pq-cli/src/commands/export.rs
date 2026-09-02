use std::io::Write;

use crate::output::Format;

pub fn run(
    files: &[String],
    output: Option<&str>,
    limit: Option<usize>,
    global_format: Format,
) -> anyhow::Result<()> {
    let opts = pq_core::reader::ReadOptions {
        limit,
        ..Default::default()
    };

    // When writing to a file, auto-detect format from extension
    // When writing to stdout, use the global -f format (default jsonl)
    match output {
        Some(output_path) => {
            let format = match std::path::Path::new(output_path)
                .extension()
                .and_then(|e| e.to_str())
            {
                Some("json") => Format::Json,
                Some("jsonl" | "ndjson") => Format::JsonLines,
                Some("csv") => Format::Csv,
                _ => Format::JsonLines,
            };
            write_to_file(files, output_path, &opts, format)
        }
        None => {
            // Write to stdout using the global format
            write_to_stdout(files, &opts, global_format)
        }
    }
}

fn write_to_file(
    files: &[String],
    output_path: &str,
    opts: &pq_core::reader::ReadOptions,
    format: Format,
) -> anyhow::Result<()> {
    // Staged write — see `pq_transform::output_guard`. The readers below run
    // lazily; creating the destination up front turned
    // `pq export a.parquet -o a.parquet` into a zero-byte file.
    let total_rows = pq_transform::output_guard::with_atomic_output(
        output_path,
        |staged| -> anyhow::Result<usize> { write_rows(files, staged, opts, format) },
    )?;

    eprintln!("Exported {total_rows} rows to {output_path}");
    Ok(())
}

fn write_rows(
    files: &[String],
    output_path: &std::path::Path,
    opts: &pq_core::reader::ReadOptions,
    format: Format,
) -> anyhow::Result<usize> {
    let mut out_file = std::fs::File::create(output_path)?;
    let mut total_rows: usize = 0;

    if format == Format::Json {
        let mut all_rows: Vec<serde_json::Value> = Vec::new();
        for f in files {
            let (batches, _schema) = pq_core::reader::open_batches(f, opts)?;
            for batch in &batches {
                let rows = pq_query::convert::batch_to_json_rows(batch);
                total_rows += rows.len();
                all_rows.extend(rows);
            }
        }
        serde_json::to_writer_pretty(&mut out_file, &all_rows)?;
        writeln!(out_file)?;
    } else if format == Format::Csv {
        // Header is the union of every file's schema field names, in
        // file/column order — not just the first file's. `export h1.parquet
        // h2.parquet` with differing schemas used to freeze the header from
        // file 1's first row, which silently shifted or dropped values from
        // files with a different key set (see the CSV column-shift bug).
        //
        // The union is built from a cheap metadata-only read per file
        // (`open_metadata` reads the Parquet footer, not row data), so this
        // pass doesn't require buffering any row data — the row-writing
        // pass below still streams file-by-file and batch-by-batch exactly
        // as before.
        let schemas: Vec<_> = files
            .iter()
            .map(|f| pq_core::reader::open_metadata(f).map(|(schema, _rows)| schema))
            .collect::<Result<_, _>>()?;
        let header = super::write_output::union_header(schemas);

        if !header.is_empty() {
            out_file.write_all(&super::write_output::csv_record_bytes(&header)?)?;
        }
        for f in files {
            let (batches, _schema) = pq_core::reader::open_batches(f, opts)?;
            for batch in &batches {
                let rows = pq_query::convert::batch_to_json_rows(batch);
                total_rows += rows.len();
                for row in &rows {
                    if let Some(obj) = row.as_object() {
                        let record = super::write_output::csv_record(&header, obj);
                        out_file.write_all(&super::write_output::csv_record_bytes(&record)?)?;
                    }
                }
            }
        }
    } else {
        for f in files {
            let (batches, _schema) = pq_core::reader::open_batches(f, opts)?;
            match format {
                Format::JsonLines | Format::Plain => {
                    for batch in &batches {
                        let rows = pq_query::convert::batch_to_json_rows(batch);
                        total_rows += rows.len();
                        for row in &rows {
                            serde_json::to_writer(&mut out_file, row)?;
                            writeln!(out_file)?;
                        }
                    }
                }
                _ => unreachable!(),
            }
        }
    }

    Ok(total_rows)
}

fn write_to_stdout(
    files: &[String],
    opts: &pq_core::reader::ReadOptions,
    format: Format,
) -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    if format == Format::Json {
        let mut all_rows: Vec<serde_json::Value> = Vec::new();
        for f in files {
            let (batches, _schema) = pq_core::reader::open_batches(f, opts)?;
            for batch in &batches {
                all_rows.extend(pq_query::convert::batch_to_json_rows(batch));
            }
        }
        serde_json::to_writer_pretty(&mut writer, &all_rows)?;
        writeln!(writer)?;
    } else if format == Format::Table {
        let mut all_batches = Vec::new();
        for f in files {
            let (batches, _schema) = pq_core::reader::open_batches(f, opts)?;
            all_batches.extend(batches);
        }
        crate::output::render_batches(&mut writer, &all_batches, format)?;
    } else {
        for f in files {
            let (batches, _schema) = pq_core::reader::open_batches(f, opts)?;
            for batch in &batches {
                for row in &pq_query::convert::batch_to_json_rows(batch) {
                    serde_json::to_writer(&mut writer, row)?;
                    writeln!(writer)?;
                }
            }
        }
    }

    Ok(())
}
