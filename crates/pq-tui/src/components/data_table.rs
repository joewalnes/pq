use arrow::datatypes::Schema;
use ratatui::prelude::*;
use ratatui::widgets::*;
use std::sync::Arc;

use crate::page_cache::PageCache;

const MIN_COL_WIDTH: u16 = 10;
const MAX_COL_WIDTH: u16 = 60;

pub struct DataTableState {
    pub headers: Vec<String>,
    pub total_rows: usize,
    pub selected_row: usize,
    pub col_offset: usize,
    pub col_widths: Vec<u16>,
}

impl DataTableState {
    pub fn new(schema: &Arc<Schema>, first_page_rows: &[Vec<String>], total_rows: usize) -> Self {
        let headers: Vec<String> = schema.fields().iter().map(|f| f.name().clone()).collect();

        // Calculate column widths based on first page content, clamped to [MIN, MAX]
        let col_widths: Vec<u16> = headers
            .iter()
            .enumerate()
            .map(|(i, h)| {
                let max_data = first_page_rows
                    .iter()
                    .map(|r| r.get(i).map(|s| s.len()).unwrap_or(0))
                    .max()
                    .unwrap_or(0);
                let content_width = h.len().max(max_data);
                // Add padding (2 chars), then clamp
                ((content_width + 2) as u16).clamp(MIN_COL_WIDTH, MAX_COL_WIDTH)
            })
            .collect();

        Self {
            headers,
            total_rows,
            selected_row: 0,
            col_offset: 0,
            col_widths,
        }
    }

    pub fn scroll_down(&mut self) {
        if self.total_rows > 0 && self.selected_row + 1 < self.total_rows {
            self.selected_row += 1;
        }
    }

    pub fn scroll_up(&mut self) {
        if self.selected_row > 0 {
            self.selected_row -= 1;
        }
    }

    pub fn scroll_left(&mut self) {
        if self.col_offset > 0 {
            self.col_offset -= 1;
        }
    }

    pub fn scroll_right(&mut self) {
        if self.col_offset + 1 < self.headers.len() {
            self.col_offset += 1;
        }
    }

    pub fn scroll_to_top(&mut self) {
        self.selected_row = 0;
    }

    pub fn scroll_to_bottom(&mut self) {
        if self.total_rows > 0 {
            self.selected_row = self.total_rows - 1;
        }
    }

    /// Determine which columns fit in the given width starting from col_offset.
    fn visible_columns(&self, available_width: u16) -> Vec<usize> {
        let mut cols = Vec::new();
        let mut used = 0u16;
        for i in self.col_offset..self.headers.len() {
            let w = self.col_widths.get(i).copied().unwrap_or(MIN_COL_WIDTH);
            // Account for table cell separator (1 char between columns)
            let needed = if cols.is_empty() { w } else { w + 1 };
            if used + needed > available_width && !cols.is_empty() {
                break;
            }
            used += needed;
            cols.push(i);
        }
        cols
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, block: Block, page_cache: &PageCache) {
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.total_rows == 0 {
            frame.render_widget(
                Paragraph::new("No data").style(Style::default().fg(Color::DarkGray)),
                inner,
            );
            return;
        }

        // Row number column: width based on digit count of total rows
        let row_num_width = (self.total_rows.max(1).ilog10() as u16 + 1).max(2) + 1; // +1 padding
        let data_width = inner.width.saturating_sub(row_num_width + 1); // +1 for separator
        let visible_cols = self.visible_columns(data_width);

        // Header: # + data columns
        let mut header_cells: Vec<Cell> = vec![Cell::from("#").style(
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )];
        header_cells.extend(visible_cols.iter().map(|&i| {
            Cell::from(self.headers[i].clone()).style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
        }));
        let header = Row::new(header_cells).height(1);

        // Rows
        let visible_height = inner.height.saturating_sub(2) as usize; // header + border
        let start = if self.selected_row >= visible_height {
            self.selected_row - visible_height + 1
        } else {
            0
        };
        let end = (start + visible_height).min(self.total_rows);

        let table_rows: Vec<Row> = (start..end)
            .map(|actual_idx| {
                let is_selected = actual_idx == self.selected_row;
                let row_num_style = if is_selected {
                    Style::default().fg(Color::White)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let mut cells: Vec<Cell> =
                    vec![Cell::from(format!("{}", actual_idx + 1)).style(row_num_style)];

                if let Some(row) = page_cache.get_row(actual_idx) {
                    cells.extend(visible_cols.iter().map(|&i| {
                        let text = row.get(i).cloned().unwrap_or_default();
                        let max = self.col_widths.get(i).copied().unwrap_or(MIN_COL_WIDTH) as usize;
                        let display = if text.len() > max.saturating_sub(1) {
                            let mut end = max.saturating_sub(2).min(text.len());
                            while end > 0 && !text.is_char_boundary(end) {
                                end -= 1;
                            }
                            format!("{}…", &text[..end])
                        } else {
                            text
                        };
                        Cell::from(display)
                    }));
                } else {
                    // Loading placeholder
                    cells.push(
                        Cell::from("Loading...").style(
                            Style::default()
                                .fg(Color::DarkGray)
                                .add_modifier(Modifier::DIM),
                        ),
                    );
                }

                let style = if actual_idx == self.selected_row {
                    Style::default().bg(Color::DarkGray).fg(Color::White)
                } else {
                    Style::default().fg(Color::White)
                };
                Row::new(cells).style(style)
            })
            .collect();

        let mut widths: Vec<Constraint> = vec![Constraint::Length(row_num_width)];
        widths.extend(visible_cols.iter().map(|&i| {
            Constraint::Length(self.col_widths.get(i).copied().unwrap_or(MIN_COL_WIDTH))
        }));

        let table = Table::new(table_rows, &widths)
            .header(header)
            .row_highlight_style(Style::default().bg(Color::DarkGray));

        frame.render_widget(table, inner);
    }

    /// Return a string describing the visible column range, for the status bar.
    pub fn column_status(&self, available_width: u16) -> String {
        let visible = self.visible_columns(available_width);
        if visible.is_empty() {
            return String::new();
        }
        let first = visible[0] + 1;
        let last = visible[visible.len() - 1] + 1;
        let total = self.headers.len();
        if first == 1 && last == total {
            String::new()
        } else {
            format!("Cols {first}-{last}/{total}")
        }
    }
}
