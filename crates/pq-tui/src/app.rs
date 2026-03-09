use std::path::PathBuf;
use std::sync::Arc;

use arrow::array::RecordBatch;
use arrow::datatypes::Schema;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::components::data_table::DataTableState;
use crate::components::schema_tree::SchemaTreeState;
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ActivePanel {
    Data,
    Schema,
    Filter,
}

pub struct App {
    pub path: PathBuf,
    pub schema: Arc<Schema>,
    pub batches: Vec<RecordBatch>,
    pub total_rows: usize,
    pub active_panel: ActivePanel,
    pub data_table: DataTableState,
    pub schema_tree: SchemaTreeState,
    pub filter_input: String,
    pub filter_active: bool,
    pub should_quit: bool,
    pub theme: Theme,
    pub status_message: String,
}

impl App {
    pub fn new(path: PathBuf, schema: Arc<Schema>, batches: Vec<RecordBatch>) -> Self {
        let total_rows: usize = batches.iter().map(|b| b.num_rows()).sum();
        let data_table = DataTableState::new(&schema, &batches);
        let schema_tree = SchemaTreeState::new(&schema);

        Self {
            path,
            schema,
            batches,
            total_rows,
            active_panel: ActivePanel::Data,
            data_table,
            schema_tree,
            filter_input: String::new(),
            filter_active: false,
            should_quit: false,
            theme: Theme::default(),
            status_message: String::new(),
        }
    }

    pub fn run(&mut self, terminal: &mut ratatui::Terminal<impl Backend>) -> anyhow::Result<()> {
        loop {
            terminal.draw(|frame| self.draw(frame))?;

            if event::poll(std::time::Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    self.handle_key(key);
                }
            }

            if self.should_quit {
                break;
            }
        }
        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if self.filter_active {
            match key.code {
                KeyCode::Esc => {
                    self.filter_active = false;
                }
                KeyCode::Enter => {
                    self.filter_active = false;
                    self.status_message = format!("Filter: {}", self.filter_input);
                }
                KeyCode::Backspace => {
                    self.filter_input.pop();
                }
                KeyCode::Char(c) => {
                    self.filter_input.push(c);
                }
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Tab => {
                self.active_panel = match self.active_panel {
                    ActivePanel::Data => ActivePanel::Schema,
                    ActivePanel::Schema => ActivePanel::Data,
                    ActivePanel::Filter => ActivePanel::Data,
                };
            }
            KeyCode::Char('/') => {
                self.filter_active = true;
                self.active_panel = ActivePanel::Filter;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.active_panel == ActivePanel::Data {
                    self.data_table.scroll_down();
                } else {
                    self.schema_tree.scroll_down();
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.active_panel == ActivePanel::Data {
                    self.data_table.scroll_up();
                } else {
                    self.schema_tree.scroll_up();
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if self.active_panel == ActivePanel::Data {
                    self.data_table.scroll_left();
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.active_panel == ActivePanel::Data {
                    self.data_table.scroll_right();
                }
            }
            KeyCode::PageDown => {
                if self.active_panel == ActivePanel::Data {
                    for _ in 0..20 {
                        self.data_table.scroll_down();
                    }
                }
            }
            KeyCode::PageUp => {
                if self.active_panel == ActivePanel::Data {
                    for _ in 0..20 {
                        self.data_table.scroll_up();
                    }
                }
            }
            KeyCode::Home | KeyCode::Char('g') => {
                if self.active_panel == ActivePanel::Data {
                    self.data_table.scroll_to_top();
                }
            }
            KeyCode::End | KeyCode::Char('G') => {
                if self.active_panel == ActivePanel::Data {
                    self.data_table.scroll_to_bottom();
                }
            }
            _ => {}
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let chunks = ratatui::layout::Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Title
                Constraint::Min(0),    // Main content
                Constraint::Length(1), // Filter / status bar
                Constraint::Length(1), // Help bar
            ])
            .split(area);

        // Title bar
        let title = format!(
            " {} | {} rows | {} columns",
            self.path.display(),
            self.total_rows,
            self.schema.fields().len()
        );
        frame.render_widget(
            Paragraph::new(title).style(Style::default().bg(Color::Blue).fg(Color::White)),
            chunks[0],
        );

        // Main content: split horizontally
        let main_chunks = ratatui::layout::Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(75), Constraint::Percentage(25)])
            .split(chunks[1]);

        // Data table
        let data_border_style = if self.active_panel == ActivePanel::Data {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let data_block = Block::default()
            .title(" Data ")
            .borders(Borders::ALL)
            .border_style(data_border_style);
        self.data_table.render(frame, main_chunks[0], data_block);

        // Schema tree
        let schema_border_style = if self.active_panel == ActivePanel::Schema {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let schema_block = Block::default()
            .title(" Schema ")
            .borders(Borders::ALL)
            .border_style(schema_border_style);
        self.schema_tree.render(frame, main_chunks[1], schema_block);

        // Filter / status
        let status_text = if self.filter_active {
            format!("Filter: {}_", self.filter_input)
        } else if !self.status_message.is_empty() {
            self.status_message.clone()
        } else {
            let row_status = format!(
                "Row {}/{}",
                self.data_table.selected_row + 1,
                self.total_rows
            );
            let data_width = main_chunks[0]
                .width
                .saturating_sub(2); // account for borders
            let col_status = self.data_table.column_status(data_width);
            if col_status.is_empty() {
                row_status
            } else {
                format!("{row_status}  {col_status}")
            }
        };
        frame.render_widget(
            Paragraph::new(status_text).style(Style::default().fg(Color::Yellow)),
            chunks[2],
        );

        // Help bar
        let help = " q:Quit  Tab:Switch Panel  j/k:Scroll  h/l:Columns  /:Filter  PgUp/PgDn:Page ";
        frame.render_widget(
            Paragraph::new(help).style(Style::default().fg(Color::DarkGray)),
            chunks[3],
        );
    }
}
