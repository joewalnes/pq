use std::path::PathBuf;

use pq_core::reader::{open_batches_with_row_count, open_metadata, ReadOptions};
use pq_core::source;
use pq_tui::page_cache::{format_batches_to_strings, Page, PAGE_SIZE};

pub fn run(file: &str) -> anyhow::Result<()> {
    let path = PathBuf::from(file);

    let (schema, total_rows, first_page) = if source::is_url(file) {
        // Remote: read metadata only (fast), let the background thread fetch data
        let (schema, total_rows) = open_metadata(file)?;
        (schema, total_rows as usize, None)
    } else {
        // Local: load first page synchronously for instant display
        let opts = ReadOptions {
            columns: None,
            limit: Some(PAGE_SIZE),
            offset: None,
            batch_size: 8192,
        };
        let (batches, schema, total_rows) = open_batches_with_row_count(file, &opts)?;
        let rows = format_batches_to_strings(&batches);
        let first_page = Page { rows, batches };
        (schema, total_rows as usize, Some(first_page))
    };

    // Set up terminal
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture,
    )?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    // A panic anywhere in `app.run` (or a widget it calls into) unwinds
    // straight past the "restore terminal" step below, leaving the user's
    // terminal in raw mode, in the alternate screen, with mouse capture on
    // and the cursor hidden — the shell echoes nothing and arrow keys emit
    // escape codes until they run `reset` blind. Install a hook that
    // restores the terminal first and *then* hands off to whatever hook was
    // already installed (so the panic message/backtrace still prints, just
    // onto a usable terminal). This only guards the panic path; the normal
    // Ok/Err return path below still does its own restore.
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        restore_terminal();
        previous_hook(panic_info);
    }));

    // Run app
    let mut app = pq_tui::app::App::new(path, file.to_string(), schema, total_rows, first_page);
    let result = app.run(&mut terminal);

    // Restore terminal (normal path — a panic is handled by the hook above
    // instead). The hook stays installed for the rest of the process: `pq
    // view` is a single subcommand that returns straight back to `main` and
    // exits, so there is no later "real" TUI session for a lingering
    // restore-then-panic hook to interfere with.
    restore_terminal();

    result
}

/// Leave raw mode, leave the alternate screen, disable mouse capture, and
/// show the cursor again. Best-effort: called from both the normal return
/// path and the panic hook, and on the panic path there is no good recovery
/// from a second failure, so errors here are swallowed rather than
/// propagated or panicked on.
fn restore_terminal() {
    let _ = crossterm::terminal::disable_raw_mode();
    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture,
    );
    let _ = crossterm::execute!(std::io::stdout(), crossterm::cursor::Show);
}
