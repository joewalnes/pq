mod cli;
mod commands;
mod files;
mod output;

use clap::{CommandFactory, Parser};

use cli::{Cli, Command};
use output::{Format, OutputMode};

fn main() {
    // If no subcommand is given but a file/URL is provided, default to `view`.
    // e.g. `pq data.parquet` behaves like `pq view data.parquet`.
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            let args: Vec<String> = std::env::args().collect();
            // Only try the `view` fallback when the first non-flag argument
            // isn't already a known subcommand.  Otherwise `pq sql` (missing
            // required arg) would be rewritten to `pq view sql` and silently
            // try to open a file called "sql".
            let first_positional = args[1..].iter().find(|a| !a.starts_with('-'));
            let is_subcommand = first_positional.is_some_and(|a| is_known_subcommand(a));

            if args.len() > 1 && !is_subcommand {
                let mut new_args = vec![args[0].clone(), "view".to_string()];
                new_args.extend(args[1..].iter().cloned());
                match Cli::try_parse_from(&new_args) {
                    Ok(cli) => cli,
                    Err(_) => e.exit(),
                }
            } else {
                e.exit()
            }
        }
    };

    if cli.debug {
        pq_core::source::set_debug(true);
    }

    let mode = OutputMode::detect(cli.output_format.as_ref());
    let format = Format::from_cli(cli.output_format.as_ref(), mode);

    let explicit_format = cli.output_format.is_some();
    let result = run(cli, format, explicit_format);

    if let Err(e) = result {
        eprintln!("Error: {e:#}");
        std::process::exit(1);
    }
}

fn is_known_subcommand(arg: &str) -> bool {
    Cli::command()
        .get_subcommands()
        .any(|cmd| cmd.get_name() == arg)
}

fn run(cli: Cli, format: Format, explicit_format: bool) -> anyhow::Result<()> {
    match cli.command {
        Command::Info { ref files } => {
            let resolved = files::resolve_files(files)?;
            for f in &resolved {
                commands::info::run(f, format)?;
            }
            Ok(())
        }

        Command::Schema {
            ref files,
            style: ref schema_fmt,
        } => {
            let resolved = files::resolve_files(files)?;
            for f in &resolved {
                commands::schema::run(f, schema_fmt, format)?;
            }
            Ok(())
        }

        Command::Stats {
            ref files,
            describe,
            top,
            sample_size,
        } => {
            let resolved = files::resolve_files(files)?;
            if describe {
                commands::describe::run(&resolved, top, sample_size, format)
            } else {
                for f in &resolved {
                    commands::stats::run(f, format)?;
                }
                Ok(())
            }
        }

        Command::Layout { ref files } => {
            let resolved = files::resolve_files(files)?;
            for f in &resolved {
                commands::layout::run(f, format)?;
            }
            Ok(())
        }

        Command::Cat {
            ref files,
            limit,
            offset,
            ref columns,
            ref where_clause,
            ref jq,
            ref output,
        } => {
            let resolved = files::resolve_files(files)?;
            // TTY mode: default limit to 1000 to prevent hanging on large files
            // (skip when writing to a file — user wants all rows)
            let effective_limit =
                if limit.is_none() && output.is_none() && console::Term::stdout().is_term() {
                    eprintln!("(showing first 1,000 rows; use --limit to override)");
                    Some(1000)
                } else {
                    limit
                };
            // When --jq is used, default to compact JSONL like the jq command
            let cat_format = if jq.is_some() && !explicit_format {
                Format::JsonLines
            } else {
                format
            };
            commands::cat::run(
                &resolved,
                effective_limit,
                offset,
                columns.clone(),
                where_clause.as_deref(),
                jq.as_deref(),
                output.as_deref(),
                cat_format,
            )
        }

        Command::Head {
            ref files,
            lines,
            ref columns,
        } => {
            let resolved = files::resolve_files(files)?;
            commands::cat::run(
                &resolved,
                Some(lines),
                None,
                columns.clone(),
                None,
                None,
                None,
                format,
            )
        }

        Command::Tail {
            ref files,
            lines,
            ref columns,
        } => {
            let resolved = files::resolve_files(files)?;
            run_tail(&resolved, lines, columns.clone(), format)
        }

        Command::Sample {
            ref files,
            lines,
            seed,
            ref columns,
        } => {
            let resolved = files::resolve_files(files)?;
            run_sample(&resolved, lines, seed, columns.clone(), format)
        }

        Command::Count { ref files } => {
            let resolved = files::resolve_files(files)?;
            commands::count::run(&resolved, format)
        }

        Command::Sql {
            ref query,
            ref output,
        } => match query.as_deref() {
            None | Some("help") => {
                Cli::command()
                    .find_subcommand_mut("sql")
                    .unwrap()
                    .print_long_help()?;
                Ok(())
            }
            Some(q) => commands::sql::run(q, output.as_deref(), format, explicit_format),
        },

        Command::View { ref file } => commands::view::run(file),

        Command::Select {
            ref file,
            ref columns,
            ref output,
        } => commands::select::run(file, columns, output),

        Command::Slice {
            ref file,
            offset,
            limit,
            ref output,
        } => commands::slice::run(file, offset, limit, output),

        Command::Merge {
            ref files,
            ref output,
            ref schema_mode,
        } => commands::merge::run(files, output, schema_mode),

        Command::Import {
            ref input,
            ref output,
            ref input_format,
        } => commands::convert::run(input, output, input_format.as_ref()),

        Command::Jq {
            ref files,
            ref filter,
            slurp,
            raw_output,
            ref output,
        } => {
            let resolved = files::resolve_files(files)?;
            // jq defaults to compact JSONL (like real jq), unless user explicitly picks a format
            let jq_format = if explicit_format {
                format
            } else {
                Format::JsonLines
            };
            commands::jq::run(
                &resolved,
                filter,
                slurp,
                raw_output,
                output.as_deref(),
                jq_format,
            )
        }

        Command::Export {
            ref files,
            ref output,
            limit,
        } => {
            let resolved = files::resolve_files(files)?;
            commands::export::run(&resolved, output.as_deref(), limit, format, explicit_format)
        }

        Command::Grep {
            ref files,
            ref pattern,
            ref columns,
            limit,
            ignore_case,
        } => {
            let resolved = files::resolve_files(files)?;
            commands::grep::run(
                &resolved,
                pattern,
                columns.clone(),
                limit,
                ignore_case,
                format,
            )
        }

        Command::Split {
            ref file,
            rows,
            ref partition_by,
            ref output,
        } => commands::split::run(file, rows, partition_by.as_deref(), output),

        Command::Validate { ref files } => {
            let resolved = files::resolve_files(files)?;
            for f in &resolved {
                commands::validate::run(f, format)?;
            }
            Ok(())
        }

        Command::Capabilities => commands::capabilities::run(format),

        Command::Completions { shell } => commands::completions::run(shell),
    }
}

/// Last N rows of the *concatenation* of `files`, in the order given — the
/// same treatment `cat`/`head` give multiple files (one logical stream), so
/// `tail` mirrors `head`'s precedent rather than inventing a per-file rule.
/// A per-file "last N of each" would silently multiply the output size by
/// the file count for the same `-n`, which is surprising for a flag whose
/// whole meaning is "how many rows do I get".
fn run_tail(
    files: &[String],
    n: usize,
    columns: Option<Vec<String>>,
    format: Format,
) -> anyhow::Result<()> {
    // Row counts are metadata-only reads (cheap) and let us work out, before
    // touching any data pages, exactly which files the last N rows fall in.
    let counts: Vec<i64> = files
        .iter()
        .map(|f| pq_core::reader::open_row_count(f).map_err(anyhow::Error::from))
        .collect::<anyhow::Result<_>>()?;
    let total: i64 = counts.iter().sum();

    let n = n.min(total.max(0) as usize) as i64;
    let global_start = total - n;

    let mut tail_batches: Vec<arrow::array::RecordBatch> = Vec::new();
    let mut file_start: i64 = 0;
    for (file, &count) in files.iter().zip(counts.iter()) {
        let file_end = file_start + count;
        // Intersect this file's [file_start, file_end) with [global_start, total).
        let lo = global_start.max(file_start);
        let hi = total.min(file_end);
        if lo < hi {
            let opts = pq_core::reader::ReadOptions {
                columns: columns.clone(),
                limit: Some((hi - lo) as usize),
                offset: Some((lo - file_start) as usize),
                batch_size: 8192,
            };
            let (batches, _schema) = pq_core::reader::open_batches(file, &opts)?;
            tail_batches.extend(batches);
        }
        file_start = file_end;
    }

    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    output::render_batches(&mut writer, &tail_batches, format)?;
    Ok(())
}

/// Uniform random sample of N rows drawn from the *concatenation* of
/// `files` (matching `count`'s "sum across files" and `cat`/`head`'s
/// "one logical stream" treatment of multiple files) rather than N rows
/// from each file, since `-n` names a total row budget, not a per-file one.
fn run_sample(
    files: &[String],
    n: usize,
    seed: Option<u64>,
    columns: Option<Vec<String>>,
    format: Format,
) -> anyhow::Result<()> {
    use rand::seq::index::sample;
    use rand::SeedableRng;

    let counts: Vec<i64> = files
        .iter()
        .map(|f| pq_core::reader::open_row_count(f).map_err(anyhow::Error::from))
        .collect::<anyhow::Result<_>>()?;
    let total = counts.iter().sum::<i64>() as usize;

    if total == 0 {
        let stdout = std::io::stdout();
        let mut writer = stdout.lock();
        output::render_batches(&mut writer, &[], format)?;
        return Ok(());
    }

    let sample_n = n.min(total);

    // Generate sorted random indices over the virtual concatenation
    // [0, total) without allocating 0..total.
    let indices: Vec<usize> = match seed {
        Some(s) => {
            let mut rng = rand::rngs::StdRng::seed_from_u64(s);
            let mut v = sample(&mut rng, total, sample_n).into_vec();
            v.sort_unstable();
            v
        }
        None => {
            let mut rng = rand::thread_rng();
            let mut v = sample(&mut rng, total, sample_n).into_vec();
            v.sort_unstable();
            v
        }
    };

    // Map each global index to (file index, local offset), then group
    // consecutive local indices *within the same file* into (offset, count)
    // ranges to minimize reads.
    let mut file_bounds: Vec<(i64, i64)> = Vec::with_capacity(counts.len()); // (start, end)
    let mut acc = 0i64;
    for &c in &counts {
        file_bounds.push((acc, acc + c));
        acc += c;
    }

    let mut ranges: Vec<(usize, usize, usize)> = Vec::new(); // (file_idx, offset, count)
    let mut file_idx = 0usize;
    for &idx in &indices {
        let idx = idx as i64;
        while idx >= file_bounds[file_idx].1 {
            file_idx += 1;
        }
        let local = (idx - file_bounds[file_idx].0) as usize;
        if let Some(last) = ranges.last_mut() {
            if last.0 == file_idx && local == last.1 + last.2 {
                last.2 += 1;
                continue;
            }
        }
        ranges.push((file_idx, local, 1));
    }

    // Read only the needed ranges and collect sampled batches, in global
    // (i.e. file) order.
    let mut sampled_batches: Vec<arrow::array::RecordBatch> = Vec::new();
    for (file_idx, offset, count) in &ranges {
        let opts = pq_core::reader::ReadOptions {
            columns: columns.clone(),
            limit: Some(*count),
            offset: Some(*offset),
            batch_size: 8192,
        };
        let (batches, _schema) = pq_core::reader::open_batches(&files[*file_idx], &opts)?;
        sampled_batches.extend(batches);
    }

    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    output::render_batches(&mut writer, &sampled_batches, format)?;

    Ok(())
}
