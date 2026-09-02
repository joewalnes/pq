use std::io::Write;
use std::path::Path;

use arrow::array::RecordBatch;

use crate::output::{self, Format};

pub fn run(
    query: &str,
    output: Option<&str>,
    format: Format,
    explicit_format: bool,
) -> anyhow::Result<()> {
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
                    write_output_file(staged, path, &batches, format, explicit_format)
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

/// The format implied by an output file's extension, distinguishing "no
/// recognized extension" from any specific format — unlike
/// `write_output::format_from_extension`, which folds the unrecognized case
/// into a silent `JsonLines` default. That silent fold is exactly the shape
/// of the confirmed PART 1 bug (`export`'s equivalent default swallowed an
/// explicit `-f csv` into JSONL written under a `.parquet` name), so
/// resolving it here needs to see "unrecognized" as its own case.
enum ExtFormat {
    Parquet,
    Text(Format),
    Unrecognized,
}

fn extension_format(path: &str) -> ExtFormat {
    match Path::new(path).extension().and_then(|e| e.to_str()) {
        Some("parquet") => ExtFormat::Parquet,
        Some("json") => ExtFormat::Text(Format::Json),
        Some("jsonl" | "ndjson") => ExtFormat::Text(Format::JsonLines),
        Some("csv") => ExtFormat::Text(Format::Csv),
        _ => ExtFormat::Unrecognized,
    }
}

enum ResolvedFormat {
    Parquet,
    Text(Format),
}

/// Which format `sql -o` writes to, and the diagnostics around the choice.
///
/// Mirrors `export::resolve_file_format` (see that module's doc comment for
/// the full reasoning) with one addition: `sql -o out.parquet` is a
/// legitimate way to materialize query results as an actual Parquet file,
/// and `-f`/`--format` has no `parquet` value to request that with — so a
/// `.parquet` extension always wins, `-f` or no `-f`.
///
/// Confirmed bug this replaces: `pq sql "..." -o out.csv -f json` used to
/// silently write CSV (extension-inferred), exit 0, no diagnostic — `-f`
/// was never consulted for file output at all. Reproduced before this fix;
/// see DIARY.md.
fn resolve_output_format(
    display_path: &str,
    global_format: Format,
    explicit_format: bool,
) -> anyhow::Result<ResolvedFormat> {
    match extension_format(display_path) {
        ExtFormat::Parquet => {
            if explicit_format {
                eprintln!(
                    "note: '{display_path}' has a .parquet extension, which always wins over \
                     -f/--format {} — there is no -f value for Parquet output",
                    format_flag_name(global_format),
                );
            }
            Ok(ResolvedFormat::Parquet)
        }
        ExtFormat::Text(ext_fmt) => {
            // Reject a format that cannot be written to a file at all
            // *before* announcing an override. `-f table -o out.csv` used to
            // print "note: -f/--format table overrides ... (csv)" and then
            // fail — a note claiming an override that never took effect.
            if explicit_format {
                check_file_format(global_format, display_path)?;
            }
            let format = if explicit_format && ext_fmt != global_format {
                eprintln!(
                    "note: -f/--format {} overrides the format implied by '{display_path}'s \
                     extension ({})",
                    format_flag_name(global_format),
                    format_flag_name(ext_fmt),
                );
                global_format
            } else {
                ext_fmt
            };
            Ok(ResolvedFormat::Text(format))
        }
        ExtFormat::Unrecognized => {
            if !explicit_format {
                anyhow::bail!(
                    "cannot determine output format for '{display_path}': its extension isn't \
                     .parquet, .json, .jsonl, or .csv. Pass -f/--format json|jsonl|csv to choose \
                     explicitly, or give the output file a recognized extension."
                );
            }
            check_file_format(global_format, display_path)?;
            Ok(ResolvedFormat::Text(global_format))
        }
    }
}

fn check_file_format(format: Format, display_path: &str) -> anyhow::Result<()> {
    if matches!(format, Format::Table | Format::Plain) {
        anyhow::bail!(
            "-f/--format {} can't be written to a file ('{display_path}') with `sql -o`; use \
             json, jsonl, or csv, or give the output file a .parquet extension",
            format_flag_name(format),
        );
    }
    Ok(())
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

/// Write the query results to `staged`, in the format implied by
/// `display_path` — the destination the *user* named.
///
/// The two arguments are deliberately different strings and that is the
/// whole point. `staged` is a temporary sibling created by
/// `pq_transform::output_guard`, whose name is derived from the destination
/// *after* symlink resolution; `display_path` is what the user typed. The
/// format must come from the latter, decided exactly once, and be handed
/// down. This used to call `write_batches_to_file(staged, ...)`, which
/// re-derived the format by sniffing `staged`'s extension — so
/// `-o link.parquet` where `link.parquet -> target.csv` staged as `...csv`,
/// the second sniff won, and `pq sql` wrote a CSV file under a `.parquet`
/// name, exit 0, "Wrote N rows". Never re-sniff a path here.
fn write_output_file(
    staged: &Path,
    display_path: &str,
    batches: &[RecordBatch],
    global_format: Format,
    explicit_format: bool,
) -> anyhow::Result<usize> {
    match resolve_output_format(display_path, global_format, explicit_format)? {
        ResolvedFormat::Parquet => super::write_output::write_batches_as(
            staged,
            batches,
            super::write_output::OutputFileFormat::Parquet,
        ),
        ResolvedFormat::Text(format) => write_text(staged, batches, format),
    }
}

fn write_text(path: &Path, batches: &[RecordBatch], format: Format) -> anyhow::Result<usize> {
    let mut file = std::fs::File::create(path)?;
    let total_rows: usize = batches.iter().map(|b| b.num_rows()).sum();

    match format {
        Format::Json => {
            let mut all_rows: Vec<serde_json::Value> = Vec::new();
            for batch in batches {
                all_rows.extend(pq_query::convert::batch_to_json_rows(batch));
            }
            serde_json::to_writer_pretty(&mut file, &all_rows)?;
            writeln!(file)?;
        }
        Format::JsonLines => {
            for batch in batches {
                for row in pq_query::convert::batch_to_json_rows(batch) {
                    serde_json::to_writer(&mut file, &row)?;
                    writeln!(file)?;
                }
            }
        }
        Format::Csv => {
            super::write_output::write_batches_csv(&mut file, batches)?;
        }
        Format::Table | Format::Plain => {
            unreachable!("check_file_format rejects Table/Plain before this is reached")
        }
    }

    Ok(total_rows)
}
