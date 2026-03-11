use ratatui::prelude::*;
use ratatui::widgets::*;
use serde_json::Value;

pub struct DetailPanelState {
    pub lines: Vec<DetailLine>,
    pub scroll_offset: usize,
}

pub struct DetailLine {
    pub spans: Vec<Span<'static>>,
}

impl Default for DetailPanelState {
    fn default() -> Self {
        Self::new()
    }
}

impl DetailPanelState {
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            scroll_offset: 0,
        }
    }

    /// Update the detail panel with a new JSON value (one row).
    pub fn update(&mut self, value: &Value) {
        self.lines.clear();
        self.scroll_offset = 0;
        if let Value::Object(map) = value {
            for (key, val) in map {
                self.render_value(key, val, 0);
            }
        }
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.scroll_offset = 0;
    }

    fn render_value(&mut self, key: &str, value: &Value, depth: usize) {
        let indent = "  ".repeat(depth);
        match value {
            Value::Object(map) => {
                self.lines.push(DetailLine {
                    spans: vec![
                        Span::raw(indent),
                        Span::styled(key.to_string(), Style::default().fg(Color::Cyan)),
                        Span::raw(":"),
                    ],
                });
                for (k, v) in map {
                    self.render_value(k, v, depth + 1);
                }
            }
            Value::Array(arr) => {
                self.lines.push(DetailLine {
                    spans: vec![
                        Span::raw(indent),
                        Span::styled(key.to_string(), Style::default().fg(Color::Cyan)),
                        Span::raw(":"),
                    ],
                });
                for (i, v) in arr.iter().enumerate() {
                    let idx_key = format!("[{i}]");
                    self.render_value(&idx_key, v, depth + 1);
                }
            }
            Value::String(s) => {
                self.lines.push(DetailLine {
                    spans: vec![
                        Span::raw(indent),
                        Span::styled(key.to_string(), Style::default().fg(Color::Cyan)),
                        Span::raw(": "),
                        Span::styled(format!("\"{s}\""), Style::default().fg(Color::Green)),
                    ],
                });
            }
            Value::Number(n) => {
                self.lines.push(DetailLine {
                    spans: vec![
                        Span::raw(indent),
                        Span::styled(key.to_string(), Style::default().fg(Color::Cyan)),
                        Span::raw(": "),
                        Span::styled(n.to_string(), Style::default().fg(Color::Yellow)),
                    ],
                });
            }
            Value::Bool(b) => {
                self.lines.push(DetailLine {
                    spans: vec![
                        Span::raw(indent),
                        Span::styled(key.to_string(), Style::default().fg(Color::Cyan)),
                        Span::raw(": "),
                        Span::styled(b.to_string(), Style::default().fg(Color::Magenta)),
                    ],
                });
            }
            Value::Null => {
                self.lines.push(DetailLine {
                    spans: vec![
                        Span::raw(indent),
                        Span::styled(key.to_string(), Style::default().fg(Color::Cyan)),
                        Span::raw(": "),
                        Span::styled("null".to_string(), Style::default().fg(Color::DarkGray)),
                    ],
                });
            }
        }
    }

    pub fn scroll_down(&mut self) {
        if self.scroll_offset + 1 < self.lines.len() {
            self.scroll_offset += 1;
        }
    }

    pub fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, block: Block) {
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.lines.is_empty() {
            frame.render_widget(
                Paragraph::new("No row selected").style(Style::default().fg(Color::DarkGray)),
                inner,
            );
            return;
        }

        let visible_height = inner.height as usize;
        let items: Vec<ListItem> = self.lines[self.scroll_offset..]
            .iter()
            .take(visible_height)
            .map(|line| ListItem::new(Line::from(line.spans.clone())))
            .collect();

        frame.render_widget(List::new(items), inner);
    }
}
