use std::path::PathBuf;

use pq_core::reader::{read_batches, ReadOptions};

pub fn run(file: &str) -> anyhow::Result<()> {
    let path = PathBuf::from(file);

    // Load initial data (first 10000 rows for TUI)
    let opts = ReadOptions {
        columns: None,
        limit: Some(10_000),
        offset: None,
        batch_size: 8192,
    };

    let (batches, schema) = read_batches(&path, &opts)?;

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
    let mut app = pq_tui::app::App::new(path, schema, batches);
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
