pub mod csv;
pub mod json;
pub mod table;

use arrow::array::RecordBatch;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether renderers (currently just the table renderer) should emit ANSI
/// color codes. Set once, early in `main()`, by `configure_color`, and read
/// by `table::render_table` -- a process-global rather than a threaded
/// parameter because `render_batches`/`render_table` are called from many
/// command modules and a signature change would ripple across files outside
/// this flag's scope.
static COLOR_ENABLED: AtomicBool = AtomicBool::new(false);

/// Pure decision function, kept separate from env/TTY lookups so it can be
/// unit-tested without mutating process-global state (env vars and the
/// `COLOR_ENABLED` flag are both process-wide, so tests that poke them
/// directly are order-dependent when run in parallel).
///
/// `--color=always`/`--color=never` are explicit user overrides and take
/// priority even over `NO_COLOR`. `--color=auto` (the default) honors the
/// no-color.org convention: a `NO_COLOR` environment variable with any
/// non-empty value disables color, and otherwise color follows whether
/// stdout is a terminal.
pub fn resolve_color(mode: &crate::cli::ColorMode, no_color_set: bool, is_tty: bool) -> bool {
    use crate::cli::ColorMode;
    match mode {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => !no_color_set && is_tty,
    }
}

/// Resolve `--color` against the real environment and store the result for
/// renderers to consult via `color_enabled()`. Must be called once, early
/// in `main()`, before any output is produced.
pub fn configure_color(mode: &crate::cli::ColorMode) {
    let no_color_set = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    let is_tty = console::Term::stdout().is_term();
    COLOR_ENABLED.store(resolve_color(mode, no_color_set, is_tty), Ordering::Relaxed);
}

pub fn color_enabled() -> bool {
    COLOR_ENABLED.load(Ordering::Relaxed)
}

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

#[cfg(test)]
mod color_tests {
    use super::resolve_color;
    use crate::cli::ColorMode;

    #[test]
    fn always_wins_over_no_color_and_non_tty() {
        assert!(resolve_color(&ColorMode::Always, true, false));
    }

    #[test]
    fn never_wins_over_tty_and_no_no_color() {
        assert!(!resolve_color(&ColorMode::Never, false, true));
    }

    #[test]
    fn auto_is_off_when_no_color_set_even_on_a_tty() {
        assert!(!resolve_color(&ColorMode::Auto, true, true));
    }

    #[test]
    fn auto_is_off_when_not_a_tty() {
        assert!(!resolve_color(&ColorMode::Auto, false, false));
    }

    #[test]
    fn auto_is_on_for_a_tty_with_no_color_unset() {
        assert!(resolve_color(&ColorMode::Auto, false, true));
    }
}
