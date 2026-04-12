use std::path::PathBuf;
use std::sync::Arc;

use arrow::datatypes::Schema;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::components::data_table::DataTableState;
use crate::components::detail_panel::DetailPanelState;
use crate::components::schema_tree::SchemaTreeState;
use crate::page_cache::{Page, PageCache};
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AppTab {
    Data,
    Schema,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LayoutMode {
    SplitHorizontal,
    SplitVertical,
    ListOnly,
    DetailOnly,
}

impl LayoutMode {
    fn next(self) -> Self {
        match self {
            Self::SplitHorizontal => Self::SplitVertical,
            Self::SplitVertical => Self::ListOnly,
            Self::ListOnly => Self::DetailOnly,
            Self::DetailOnly => Self::SplitHorizontal,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::SplitHorizontal => "H-Split",
            Self::SplitVertical => "V-Split",
            Self::ListOnly => "List",
            Self::DetailOnly => "Detail",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DataFocus {
    RowList,
    Detail,
}

pub struct App {
    pub path: PathBuf,
    pub schema: Arc<Schema>,
    pub page_cache: PageCache,
    pub total_rows: usize,
    pub tab: AppTab,
    pub layout_mode: LayoutMode,
    pub data_focus: DataFocus,
    pub data_table: DataTableState,
    pub detail_panel: DetailPanelState,
    pub schema_tree: SchemaTreeState,
    pub filter_input: String,
    pub filter_active: bool,
    pub should_quit: bool,
    pub theme: Theme,
    pub status_message: String,
    last_selected_row: usize,
}

impl App {
    pub fn new(
        path: PathBuf,
        location: String,
        schema: Arc<Schema>,
        total_rows: usize,
        first_page: Option<Page>,
    ) -> Self {
        let first_page_rows: &[Vec<String>] = match &first_page {
            Some(p) => &p.rows,
            None => &[],
        };
        let data_table = DataTableState::new(&schema, first_page_rows, total_rows);
        let schema_tree = SchemaTreeState::new(&schema);
        let mut detail_panel = DetailPanelState::new();

        let page_cache = PageCache::new(location, schema.clone(), total_rows, first_page);

        // Initialize detail panel with first row if available
        if let Some((batch, row_in_batch)) = page_cache.get_batch_row(0) {
            let json = pq_query::convert::batch_row_to_json(batch, row_in_batch);
            detail_panel.update(&json);
        }

        Self {
            path,
            schema,
            page_cache,
            total_rows,
            tab: AppTab::Data,
            layout_mode: LayoutMode::SplitHorizontal,
            data_focus: DataFocus::RowList,
            data_table,
            detail_panel,
            schema_tree,
            filter_input: String::new(),
            filter_active: false,
            should_quit: false,
            theme: Theme::default(),
            status_message: String::new(),
            last_selected_row: 0,
        }
    }

    pub fn run(&mut self, terminal: &mut ratatui::Terminal<impl Backend>) -> anyhow::Result<()> {
        loop {
            self.page_cache.poll_fetches();
            self.page_cache
                .ensure_pages_around(self.data_table.selected_row);
            self.update_detail_if_needed();

            terminal.draw(|frame| self.draw(frame))?;

            if event::poll(std::time::Duration::from_millis(50))? {
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

    /// Update detail panel when the selected row changes or its page becomes available.
    fn update_detail_if_needed(&mut self) {
        let selected = self.data_table.selected_row;
        if let Some((batch, row_in_batch)) = self.page_cache.get_batch_row(selected) {
            if selected != self.last_selected_row || self.detail_panel.lines.is_empty() {
                self.last_selected_row = selected;
                let json = pq_query::convert::batch_row_to_json(batch, row_in_batch);
                self.detail_panel.update(&json);
            }
        } else {
            self.last_selected_row = selected;
            self.detail_panel.clear();
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        // Filter input mode
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
            // Quit
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }

            // Tab: switch between Data and Schema tabs
            KeyCode::Tab => {
                self.tab = match self.tab {
                    AppTab::Data => AppTab::Schema,
                    AppTab::Schema => AppTab::Data,
                };
            }

            // v: cycle layout mode (Data tab only)
            KeyCode::Char('v') => {
                if self.tab == AppTab::Data {
                    self.layout_mode = self.layout_mode.next();
                }
            }

            // Enter: toggle focus between row list and detail panel (Data tab)
            KeyCode::Enter => {
                if self.tab == AppTab::Data {
                    self.data_focus = match self.data_focus {
                        DataFocus::RowList => DataFocus::Detail,
                        DataFocus::Detail => DataFocus::RowList,
                    };
                }
            }

            // Filter
            KeyCode::Char('/') => {
                self.filter_active = true;
            }

            // Vertical scrolling
            KeyCode::Down | KeyCode::Char('j') => match self.tab {
                AppTab::Data => match self.data_focus {
                    DataFocus::RowList => {
                        self.data_table.scroll_down();
                    }
                    DataFocus::Detail => {
                        self.detail_panel.scroll_down();
                    }
                },
                AppTab::Schema => {
                    self.schema_tree.scroll_down();
                }
            },
            KeyCode::Up | KeyCode::Char('k') => match self.tab {
                AppTab::Data => match self.data_focus {
                    DataFocus::RowList => {
                        self.data_table.scroll_up();
                    }
                    DataFocus::Detail => {
                        self.detail_panel.scroll_up();
                    }
                },
                AppTab::Schema => {
                    self.schema_tree.scroll_up();
                }
            },

            // Horizontal scrolling (row list only)
            KeyCode::Left | KeyCode::Char('h') => {
                if self.tab == AppTab::Data && self.data_focus == DataFocus::RowList {
                    self.data_table.scroll_left();
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.tab == AppTab::Data && self.data_focus == DataFocus::RowList {
                    self.data_table.scroll_right();
                }
            }

            // Page scrolling
            KeyCode::PageDown => {
                if self.tab == AppTab::Data && self.data_focus == DataFocus::RowList {
                    for _ in 0..20 {
                        self.data_table.scroll_down();
                    }
                }
            }
            KeyCode::PageUp => {
                if self.tab == AppTab::Data && self.data_focus == DataFocus::RowList {
                    for _ in 0..20 {
                        self.data_table.scroll_up();
                    }
                }
            }
            KeyCode::Home | KeyCode::Char('g') => {
                if self.tab == AppTab::Data && self.data_focus == DataFocus::RowList {
                    self.data_table.scroll_to_top();
                }
            }
            KeyCode::End | KeyCode::Char('G') => {
                if self.tab == AppTab::Data && self.data_focus == DataFocus::RowList {
                    self.data_table.scroll_to_bottom();
                }
            }
            _ => {}
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();

        // Paint the entire background so cells without explicit bg don't
        // inherit the terminal default (often black), causing a patchwork
        // of black/grey on some terminals.
        frame.render_widget(
            Block::default().style(Style::default().bg(Color::Rgb(30, 30, 30))),
            area,
        );

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Tab bar
                Constraint::Min(0),    // Main content
                Constraint::Length(1), // Status bar
                Constraint::Length(1), // Help bar
            ])
            .split(area);

        self.draw_tab_bar(frame, chunks[0]);

        match self.tab {
            AppTab::Data => self.draw_data_tab(frame, chunks[1]),
            AppTab::Schema => self.draw_schema_tab(frame, chunks[1]),
        }

        self.draw_status_bar(frame, chunks[1].width, chunks[2]);
        self.draw_help_bar(frame, chunks[3]);
    }

    fn draw_tab_bar(&self, frame: &mut Frame, area: Rect) {
        let data_style = if self.tab == AppTab::Data {
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let schema_style = if self.tab == AppTab::Schema {
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let file_info = format!(
            " {} | {} rows | {} cols",
            self.path.file_name().unwrap_or_default().to_string_lossy(),
            self.total_rows,
            self.schema.fields().len(),
        );

        let tabs = Line::from(vec![
            Span::styled(" Data ", data_style),
            Span::raw(" "),
            Span::styled(" Schema ", schema_style),
            Span::styled(file_info, Style::default().fg(Color::DarkGray)),
        ]);

        frame.render_widget(
            Paragraph::new(tabs).style(Style::default().bg(Color::Rgb(30, 30, 30))),
            area,
        );
    }

    fn draw_data_tab(&self, frame: &mut Frame, area: Rect) {
        match self.layout_mode {
            LayoutMode::SplitHorizontal => {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(area);
                self.draw_row_list(frame, chunks[0]);
                self.draw_detail_panel(frame, chunks[1]);
            }
            LayoutMode::SplitVertical => {
                let chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(area);
                self.draw_row_list(frame, chunks[0]);
                self.draw_detail_panel(frame, chunks[1]);
            }
            LayoutMode::ListOnly => {
                self.draw_row_list(frame, area);
            }
            LayoutMode::DetailOnly => {
                self.draw_detail_panel(frame, area);
            }
        }
    }

    fn draw_row_list(&self, frame: &mut Frame, area: Rect) {
        let is_focused = self.tab == AppTab::Data && self.data_focus == DataFocus::RowList;
        let border_style = if is_focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let block = Block::default()
            .title(" Rows ")
            .borders(Borders::ALL)
            .border_style(border_style);
        self.data_table.render(frame, area, block, &self.page_cache);
    }

    fn draw_detail_panel(&self, frame: &mut Frame, area: Rect) {
        let is_focused = self.tab == AppTab::Data && self.data_focus == DataFocus::Detail;
        let border_style = if is_focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let title = format!(
            " Detail — Row {}/{} ",
            self.data_table.selected_row + 1,
            self.total_rows,
        );
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(border_style);
        self.detail_panel.render(frame, area, block);
    }

    fn draw_schema_tab(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(" Schema ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));
        self.schema_tree.render(frame, area, block);
    }

    fn draw_status_bar(&self, frame: &mut Frame, main_width: u16, area: Rect) {
        let status_text = if self.filter_active {
            format!("Filter: {}_", self.filter_input)
        } else if !self.status_message.is_empty() {
            self.status_message.clone()
        } else if self.tab == AppTab::Data {
            let row_status = format!(
                "Row {}/{}",
                self.data_table.selected_row + 1,
                self.total_rows,
            );
            let layout_label = self.layout_mode.label();
            let focus_label = match self.data_focus {
                DataFocus::RowList => "rows",
                DataFocus::Detail => "detail",
            };
            let data_width = main_width.saturating_sub(2);
            let col_status = self.data_table.column_status(data_width);
            let mut parts = vec![row_status];
            if self.page_cache.is_loading() {
                parts.push("\u{27f3} Loading...".to_string());
            }
            if !col_status.is_empty() {
                parts.push(col_status);
            }
            parts.push(format!("[{layout_label}:{focus_label}]"));
            parts.join("  ")
        } else {
            "Schema view".to_string()
        };

        let style = if self.page_cache.is_loading() {
            Style::default().fg(Color::Rgb(255, 165, 0)) // orange
        } else {
            Style::default().fg(Color::Yellow)
        };

        frame.render_widget(Paragraph::new(status_text).style(style), area);
    }

    fn draw_help_bar(&self, frame: &mut Frame, area: Rect) {
        let help = match self.tab {
            AppTab::Data => {
                " q:Quit  Tab:Schema  v:Layout  Enter:Focus  j/k:Scroll  h/l:Columns  /:Filter "
            }
            AppTab::Schema => " q:Quit  Tab:Data  j/k:Scroll ",
        };
        frame.render_widget(
            Paragraph::new(help).style(Style::default().fg(Color::DarkGray)),
            area,
        );
    }
}
