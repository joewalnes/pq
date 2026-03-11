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

    // Run app
    let mut app = pq_tui::app::App::new(path, file.to_string(), schema, total_rows, first_page);
    let result = app.run(&mut terminal);

    // Restore terminal
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture,
    )?;
    terminal.show_cursor()?;

    result
}
