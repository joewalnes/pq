pub mod csv;
pub mod json;
pub mod table;

use arrow::array::RecordBatch;
use std::io::Write;

/// Detected output mode based on terminal/pipe and user flags
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputMode {
    /// TTY: pretty tables, colors, pager
    Interactive,
    /// Pipe: JSON lines, no color
    Machine,
}

impl OutputMode {
    pub fn detect(override_format: Option<&crate::cli::OutputFormat>) -> Self {
        if override_format.is_some() {
            return OutputMode::Machine;
        }
        if console::Term::stdout().is_term()
            || std::env::var_os("PQ_FORCE_TTY").is_some_and(|v| v == "1")
        {
            OutputMode::Interactive
        } else {
            OutputMode::Machine
        }
    }
}

/// Format specifier for actual output rendering
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Format {
    Json,
    JsonLines,
    Csv,
    Table,
    Plain,
}

impl Format {
    pub fn from_cli(output_format: Option<&crate::cli::OutputFormat>, mode: OutputMode) -> Self {
        match output_format {
            Some(crate::cli::OutputFormat::Json) => Format::Json,
            Some(crate::cli::OutputFormat::Jsonl) => Format::JsonLines,
            Some(crate::cli::OutputFormat::Csv) => Format::Csv,
            Some(crate::cli::OutputFormat::Table) => Format::Table,
            Some(crate::cli::OutputFormat::Plain) => Format::Plain,
            None => match mode {
                OutputMode::Interactive => Format::Table,
                OutputMode::Machine => Format::JsonLines,
            },
        }
    }
}

pub fn render_batches(
    writer: &mut dyn Write,
    batches: &[RecordBatch],
    format: Format,
) -> std::io::Result<()> {
    match format {
        Format::Json => json::render_json(writer, batches),
        Format::JsonLines => json::render_jsonl(writer, batches),
        Format::Csv => csv::render_csv(writer, batches),
        Format::Table => table::render_table(writer, batches),
        Format::Plain => table::render_plain(writer, batches),
    }
}

pub fn render_value(
    writer: &mut dyn Write,
    value: &serde_json::Value,
    format: Format,
) -> std::io::Result<()> {
    match format {
        Format::Json => {
            serde_json::to_writer_pretty(&mut *writer, value)?;
            writeln!(writer)?;
            Ok(())
        }
        Format::JsonLines | Format::Plain => {
            serde_json::to_writer(&mut *writer, value)?;
            writeln!(writer)?;
            Ok(())
        }
        Format::Table => {
            serde_json::to_writer_pretty(&mut *writer, value)?;
            writeln!(writer)?;
            Ok(())
        }
        Format::Csv => {
            serde_json::to_writer(&mut *writer, value)?;
            writeln!(writer)?;
            Ok(())
        }
    }
}
