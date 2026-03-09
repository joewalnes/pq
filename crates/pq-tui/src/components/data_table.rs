use arrow::array::RecordBatch;
use arrow::datatypes::Schema;
use arrow::util::display::ArrayFormatter;
use ratatui::prelude::*;
use ratatui::widgets::*;
use std::sync::Arc;

pub struct DataTableState {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub selected_row: usize,
    pub col_offset: usize,
    pub col_widths: Vec<u16>,
}

impl DataTableState {
    pub fn new(schema: &Arc<Schema>, batches: &[RecordBatch]) -> Self {
        let headers: Vec<String> = schema.fields().iter().map(|f| f.name().clone()).collect();

        let mut rows = Vec::new();
        for batch in batches {
            for row_idx in 0..batch.num_rows() {
                let mut row = Vec::new();
                for col_idx in 0..batch.num_columns() {
                    let col = batch.column(col_idx);
                    let formatter = ArrayFormatter::try_new(col.as_ref(), &Default::default());
                    let val = match formatter {
                        Ok(f) => f.value(row_idx).to_string(),
                        Err(_) => "<error>".to_string(),
                    };
                    row.push(val);
                }
                rows.push(row);
            }
        }

        // Calculate column widths
        let col_widths: Vec<u16> = headers
            .iter()
            .enumerate()
            .map(|(i, h)| {
                let max_data = rows
                    .iter()
                    .map(|r| r.get(i).map(|s| s.len()).unwrap_or(0))
                    .max()
                    .unwrap_or(0);
                (h.len().max(max_data).min(40) + 2) as u16
            })
            .collect();

        Self {
            headers,
            rows,
            selected_row: 0,
            col_offset: 0,
            col_widths,
        }
    }

    pub fn scroll_down(&mut self) {
        if self.selected_row + 1 < self.rows.len() {
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
        if !self.rows.is_empty() {
            self.selected_row = self.rows.len() - 1;
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, block: Block) {
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.rows.is_empty() {
            frame.render_widget(
                Paragraph::new("No data").style(Style::default().fg(Color::DarkGray)),
                inner,
            );
            return;
        }

        let visible_cols: Vec<usize> = (self.col_offset..self.headers.len()).collect();

        // Header
        let header_cells: Vec<Cell> = visible_cols
            .iter()
            .map(|&i| {
                Cell::from(self.headers[i].clone()).style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
            })
            .collect();
        let header = Row::new(header_cells).height(1);

        // Rows
        let visible_height = inner.height.saturating_sub(2) as usize; // header + border
        let start = if self.selected_row >= visible_height {
            self.selected_row - visible_height + 1
        } else {
            0
        };

        let table_rows: Vec<Row> = self.rows[start..]
            .iter()
            .take(visible_height)
            .enumerate()
            .map(|(display_idx, row)| {
                let actual_idx = start + display_idx;
                let cells: Vec<Cell> = visible_cols
                    .iter()
                    .map(|&i| Cell::from(row.get(i).cloned().unwrap_or_default()))
                    .collect();
                let style = if actual_idx == self.selected_row {
                    Style::default().bg(Color::DarkGray).fg(Color::White)
                } else {
                    Style::default()
                };
                Row::new(cells).style(style)
            })
            .collect();

        let widths: Vec<Constraint> = visible_cols
            .iter()
            .map(|&i| Constraint::Length(self.col_widths.get(i).copied().unwrap_or(10)))
            .collect();

        let table = Table::new(table_rows, &widths)
            .header(header)
            .row_highlight_style(Style::default().bg(Color::DarkGray));

        frame.render_widget(table, inner);
    }
}
