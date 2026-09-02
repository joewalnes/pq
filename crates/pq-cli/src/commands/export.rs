use std::io::Write;

use crate::output::Format;

pub fn run(
    files: &[String],
    output: Option<&str>,
    limit: Option<usize>,
    global_format: Format,
    explicit_format: bool,
) -> anyhow::Result<()> {
    let opts = pq_core::reader::ReadOptions {
        limit,
        ..Default::default()
    };

    match output {
        Some(output_path) => {
            let format = resolve_file_format(output_path, global_format, explicit_format)?;
            write_to_file(files, output_path, &opts, format)
        }
        None => {
            // Write to stdout using the global format (defaults to jsonl
            // when -f isn't given; see `Format::from_cli`).
            write_to_stdout(files, &opts, global_format)
        }
    }
}

/// Which format to write when `export` is writing to a *file*.
///
/// `-f`/`--format` is a global flag shared with every other command's
/// stdout rendering, so its *default* value (jsonl in a pipe, table on a
/// TTY — see `Format::from_cli`) means nothing about user intent for a file
/// target: the output file's extension governs by default, matching every
/// other file-writing command (`cat -O`, `jq -o`).
///
/// But a user who *explicitly* types `-f csv` has stated a real intent, and
/// silently discarding it is exactly the confirmed bug this fixes:
/// `pq export data.parquet -o a.parquet -f csv` used to write JSONL into a
/// file named `.parquet`, exit 0, with no diagnostic at all — because the
/// unrecognized `.parquet` extension fell through to a silent JsonLines
/// default and `-f csv` was never consulted for file output. So:
///
/// - extension recognized, `-f` not given (or given but agreeing): use the
///   extension, silently — this is the overwhelmingly common case and
///   should stay quiet.
/// - extension recognized, `-f` explicitly given and disagreeing: the
///   explicit flag wins, but a stderr note says so, so the loser is never
///   silent.
/// - extension unrecognized/absent, `-f` explicitly given: use `-f`.
/// - extension unrecognized/absent, `-f` not given: error. There is no
///   correct silent default here — the old JsonLines fallback is exactly
///   what produced the confirmed bug.
fn resolve_file_format(
    output_path: &str,
    global_format: Format,
    explicit_format: bool,
) -> anyhow::Result<Format> {
    let ext_format = extension_format(output_path);
    let format = match (ext_format, explicit_format) {
        (Some(ext_fmt), true) if ext_fmt != global_format => {
            eprintln!(
                "note: -f/--format {} overrides the format implied by '{output_path}'s extension ({})",
                format_flag_name(global_format),
                format_flag_name(ext_fmt),
            );
            global_format
        }
        (Some(ext_fmt), _) => ext_fmt,
        (None, true) => global_format,
        (None, false) => anyhow::bail!(
            "cannot determine export format for '{output_path}': its extension isn't \
             .json, .jsonl, or .csv. Pass -f/--format json|jsonl|csv to choose \
             explicitly, or give the output file one of those extensions."
        ),
    };
    if matches!(format, Format::Table | Format::Plain) {
        anyhow::bail!(
            "-f/--format {} can't be written to a file with `export`; use json, jsonl, or csv",
            format_flag_name(format),
        );
    }
    Ok(format)
}

/// Format implied by a file's extension, or `None` if it isn't one of the
/// formats `export` can produce (this includes `.parquet` — `export`'s job
/// is Parquet-to-other, not Parquet-to-Parquet, so that extension is just
/// as unrecognized as `.txt` or no extension at all).
fn extension_format(path: &str) -> Option<Format> {
    match std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
    {
        Some("json") => Some(Format::Json),
        Some("jsonl" | "ndjson") => Some(Format::JsonLines),
        Some("csv") => Some(Format::Csv),
        _ => None,
    }
}

/// Render a `Format` the way users typed it on the command line, for
/// diagnostics — `Format`'s `Debug` output (`JsonLines`) doesn't match the
/// `-f`/`--format` vocabulary (`jsonl`).
fn format_flag_name(format: Format) -> &'static str {
    match format {
        Format::Json => "json",
        Format::JsonLines => "jsonl",
        Format::Csv => "csv",
        Format::Table => "table",
        Format::Plain => "plain",
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
        total_rows = write_csv(files, opts, &mut out_file)?;
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

/// Write every file's rows as CSV to `writer`. Shared by the to-file and
/// to-stdout paths so `export -f csv` behaves identically either way.
///
/// Header is the union of every file's schema field names, in file/column
/// order — not just the first file's. `export h1.parquet h2.parquet` with
/// differing schemas used to freeze the header from file 1's first row,
/// which silently shifted or dropped values from files with a different
/// key set (see the CSV column-shift bug).
///
/// The union is built from a cheap metadata-only read per file
/// (`open_metadata` reads the Parquet footer, not row data), so this pass
/// doesn't require buffering any row data — the row-writing pass below
/// still streams file-by-file and batch-by-batch.
fn write_csv(
    files: &[String],
    opts: &pq_core::reader::ReadOptions,
    writer: &mut dyn Write,
) -> anyhow::Result<usize> {
    let mut total_rows: usize = 0;
    let schemas: Vec<_> = files
        .iter()
        .map(|f| pq_core::reader::open_metadata(f).map(|(schema, _rows)| schema))
        .collect::<Result<_, _>>()?;
    let header = super::write_output::union_header(schemas);

    if !header.is_empty() {
        writer.write_all(&super::write_output::csv_record_bytes(&header)?)?;
    }
    for f in files {
        let (batches, _schema) = pq_core::reader::open_batches(f, opts)?;
        for batch in &batches {
            let rows = pq_query::convert::batch_to_json_rows(batch);
            total_rows += rows.len();
            for row in &rows {
                if let Some(obj) = row.as_object() {
                    let record = super::write_output::csv_record(&header, obj);
                    writer.write_all(&super::write_output::csv_record_bytes(&record)?)?;
                }
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
    } else if format == Format::Csv {
        // Previously missing entirely: this branch used to be absorbed by
        // the JsonLines/Plain catch-all below, so `pq export data.parquet
        // -f csv` (to stdout) silently printed JSONL instead of CSV. See
        // the PART 1 bug report — "-f csv to stdout emits JSONL".
        write_csv(files, opts, &mut writer)?;
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
