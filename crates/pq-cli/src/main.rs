mod cli;
mod commands;
mod output;

use clap::Parser;
use std::io::Write;

use cli::{Cli, Command};
use output::{Format, OutputMode};

fn main() {
    // If no subcommand is given but a file/URL is provided, default to `view`.
    // e.g. `pq data.parquet` behaves like `pq view data.parquet`.
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            let args: Vec<String> = std::env::args().collect();
            if args.len() > 1 {
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

    let result = run(cli, format);

    if let Err(e) = result {
        eprintln!("Error: {e:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli, format: Format) -> anyhow::Result<()> {
    match cli.command {
        Command::Info { ref file } => commands::info::run(file, format),

        Command::Schema {
            ref file,
            format: ref schema_fmt,
        } => commands::schema::run(file, schema_fmt, format),

        Command::Stats { ref file } => commands::stats::run(file, format),

        Command::Layout { ref file } => commands::layout::run(file, format),

        Command::Cat {
            ref file,
            limit,
            offset,
            ref columns,
            ref where_clause,
            ref jq,
        } => commands::cat::run(
            file,
            limit,
            offset,
            columns.clone(),
            where_clause.as_deref(),
            jq.as_deref(),
            format,
        ),

        Command::Head {
            ref file,
            lines,
            ref columns,
        } => commands::cat::run(file, Some(lines), None, columns.clone(), None, None, format),

        Command::Tail {
            ref file,
            lines,
            ref columns,
        } => {
            let (batches, _schema) = pq_core::reader::open_tail(file, lines, columns.clone())?;
            let stdout = std::io::stdout();
            let mut writer = stdout.lock();
            output::render_batches(&mut writer, &batches, format)?;
            Ok(())
        }

        Command::Sample {
            ref file,
            lines,
            seed,
            ref columns,
        } => run_sample(file, lines, seed, columns.clone(), format),

        Command::Count { ref files } => commands::count::run(files, format),

        Command::Sql { ref query } => commands::sql::run(query, format),

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

        Command::Convert {
            ref input,
            ref output,
            ref format,
        } => commands::convert::run(input, output, format.as_ref()),

        Command::Jq {
            ref file,
            ref filter,
            slurp,
            raw_output,
        } => commands::jq::run(file, filter, slurp, raw_output, format),

        Command::Capabilities => commands::capabilities::run(format),

        Command::Completions { shell } => commands::completions::run(shell),
    }
}

fn run_sample(
    file: &str,
    n: usize,
    seed: Option<u64>,
    columns: Option<Vec<String>>,
    format: Format,
) -> anyhow::Result<()> {
    use rand::seq::SliceRandom;
    use rand::SeedableRng;

    let total = pq_core::reader::open_row_count(file)? as usize;

    if total == 0 {
        let stdout = std::io::stdout();
        let mut writer = stdout.lock();
        output::render_batches(&mut writer, &[], format)?;
        return Ok(());
    }

    let mut indices: Vec<usize> = (0..total).collect();
    match seed {
        Some(s) => {
            let mut rng = rand::rngs::StdRng::seed_from_u64(s);
            indices.shuffle(&mut rng);
        }
        None => {
            let mut rng = rand::thread_rng();
            indices.shuffle(&mut rng);
        }
    }
    indices.truncate(n);
    indices.sort_unstable();

    let opts = pq_core::reader::ReadOptions {
        columns,
        limit: None,
        offset: None,
        batch_size: 8192,
    };
    let (batches, _schema) = pq_core::reader::open_batches(file, &opts)?;

    let all_rows: Vec<serde_json::Value> = batches
        .iter()
        .flat_map(pq_query::convert::batch_to_json_rows)
        .collect();

    let sampled: Vec<serde_json::Value> = indices
        .iter()
        .filter_map(|&i| all_rows.get(i).cloned())
        .collect();

    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    match format {
        Format::Json => {
            serde_json::to_writer_pretty(&mut writer, &sampled)?;
            writeln!(writer)?;
        }
        _ => {
            for row in &sampled {
                serde_json::to_writer(&mut writer, row)?;
                writeln!(writer)?;
            }
        }
    }

    Ok(())
}
