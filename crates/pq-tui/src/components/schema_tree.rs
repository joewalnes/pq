use arrow::datatypes::{DataType, Schema};
use ratatui::prelude::*;
use ratatui::widgets::*;
use std::sync::Arc;

pub struct SchemaTreeState {
    pub items: Vec<SchemaTreeItem>,
    pub selected: usize,
}

pub struct SchemaTreeItem {
    pub name: String,
    pub type_name: String,
    pub nullable: bool,
    pub depth: usize,
}

impl SchemaTreeState {
    pub fn new(schema: &Arc<Schema>) -> Self {
        let mut items = Vec::new();
        for field in schema.fields() {
            add_field_items(
                &mut items,
                field.name(),
                field.data_type(),
                field.is_nullable(),
                0,
            );
        }
        Self { items, selected: 0 }
    }

    pub fn scroll_down(&mut self) {
        if self.selected + 1 < self.items.len() {
            self.selected += 1;
        }
    }

    pub fn scroll_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, block: Block) {
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let visible_height = inner.height as usize;
        let start = if self.selected >= visible_height {
            self.selected - visible_height + 1
        } else {
            0
        };
        // `start` is derived from `selected`, which tracks the item count at
        // the time it was last moved. If the schema is empty (a valid, if
        // unusual, Parquet file can have zero columns) `items` is empty too,
        // but `visible_height` can still be 0 (a collapsed/bordered pane),
        // making `start` come out to 1 — one past an empty slice. Clamp
        // rather than let `items[start..]` panic on an out-of-bounds start.
        let start = start.min(self.items.len());

        let items: Vec<ListItem> = self.items[start..]
            .iter()
            .take(visible_height)
            .enumerate()
            .map(|(display_idx, item)| {
                let actual_idx = start + display_idx;
                let indent = "  ".repeat(item.depth);
                let nullable = if item.nullable { "?" } else { "" };
                let text = format!("{}{}: {}{}", indent, item.name, item.type_name, nullable);
                let style = if actual_idx == self.selected {
                    Style::default().bg(Color::DarkGray).fg(Color::White)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(text).style(style)
            })
            .collect();

        frame.render_widget(List::new(items), inner);
    }
}

fn add_field_items(
    items: &mut Vec<SchemaTreeItem>,
    name: &str,
    dt: &DataType,
    nullable: bool,
    depth: usize,
) {
    match dt {
        DataType::Struct(fields) => {
            items.push(SchemaTreeItem {
                name: name.to_string(),
                type_name: "struct".to_string(),
                nullable,
                depth,
            });
            for field in fields {
                add_field_items(
                    items,
                    field.name(),
                    field.data_type(),
                    field.is_nullable(),
                    depth + 1,
                );
            }
        }
        DataType::List(inner) | DataType::LargeList(inner) => {
            let type_label = format_short_type(dt);
            items.push(SchemaTreeItem {
                name: name.to_string(),
                type_name: type_label,
                nullable,
                depth,
            });
            // Recurse into struct children of lists
            if let DataType::Struct(fields) = inner.data_type() {
                for field in fields {
                    add_field_items(
                        items,
                        field.name(),
                        field.data_type(),
                        field.is_nullable(),
                        depth + 1,
                    );
                }
            }
        }
        DataType::FixedSizeList(inner, size) => {
            let inner_type = format_short_type(inner.data_type());
            items.push(SchemaTreeItem {
                name: name.to_string(),
                type_name: format!("fixed_list<{inner_type}, {size}>"),
                nullable,
                depth,
            });
            if let DataType::Struct(fields) = inner.data_type() {
                for field in fields {
                    add_field_items(
                        items,
                        field.name(),
                        field.data_type(),
                        field.is_nullable(),
                        depth + 1,
                    );
                }
            }
        }
        DataType::Map(entry_field, _) => {
            items.push(SchemaTreeItem {
                name: name.to_string(),
                type_name: "map".to_string(),
                nullable,
                depth,
            });
            if let DataType::Struct(fields) = entry_field.data_type() {
                for field in fields {
                    add_field_items(
                        items,
                        field.name(),
                        field.data_type(),
                        field.is_nullable(),
                        depth + 1,
                    );
                }
            }
        }
        dt => {
            items.push(SchemaTreeItem {
                name: name.to_string(),
                type_name: format_short_type(dt),
                nullable,
                depth,
            });
        }
    }
}

fn format_short_type(dt: &DataType) -> String {
    match dt {
        DataType::Null => "null".to_string(),
        DataType::Boolean => "bool".to_string(),
        DataType::Int8 => "int8".to_string(),
        DataType::Int16 => "int16".to_string(),
        DataType::Int32 => "int32".to_string(),
        DataType::Int64 => "int64".to_string(),
        DataType::UInt8 => "uint8".to_string(),
        DataType::UInt16 => "uint16".to_string(),
        DataType::UInt32 => "uint32".to_string(),
        DataType::UInt64 => "uint64".to_string(),
        DataType::Float16 => "f16".to_string(),
        DataType::Float32 => "f32".to_string(),
        DataType::Float64 => "f64".to_string(),
        DataType::Utf8 | DataType::LargeUtf8 => "string".to_string(),
        DataType::Binary | DataType::LargeBinary => "binary".to_string(),
        DataType::Date32 | DataType::Date64 => "date".to_string(),
        DataType::Timestamp(_, _) => "timestamp".to_string(),
        DataType::Decimal128(p, s) | DataType::Decimal256(p, s) => format!("decimal({p},{s})"),
        DataType::List(inner) | DataType::LargeList(inner) => {
            format!("list<{}>", format_short_type(inner.data_type()))
        }
        DataType::Struct(_) => "struct".to_string(),
        DataType::Map(_, _) => "map".to_string(),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::widgets::{Block, Borders};
    use ratatui::Terminal;

    /// A schema with zero columns is a valid Parquet file (pyarrow will
    /// happily write `pa.schema([])`), so `SchemaTreeState::new` can
    /// legitimately produce an empty `items` list. Pair that with a pane
    /// squeezed down to zero inner rows (a bordered block whose outer area
    /// is 0 or 1 tall — easy to hit by resizing a terminal) and `start`
    /// computes to 1 while `items.len()` is 0: `items[1..]` on an empty
    /// slice. This must not panic.
    #[test]
    fn render_empty_schema_in_zero_height_pane_does_not_panic() {
        let state = SchemaTreeState {
            items: Vec::new(),
            selected: 0,
        };
        let backend = TestBackend::new(20, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        let area = Rect::new(0, 0, 20, 1);
        let block = Block::default().borders(Borders::ALL);
        // Sanity check: a 1-row area with borders on all sides has 0 inner
        // rows, which is the condition that makes `start` overshoot.
        assert_eq!(block.inner(area).height, 0);

        terminal
            .draw(|frame| {
                state.render(frame, area, block);
            })
            .unwrap();
    }

    /// Same shape of bug: `selected` inconsistent with `items` (`scroll_down`
    /// never lets this happen today, but the render function shouldn't rely
    /// on that as its only defense — a future reload-on-file-switch feature
    /// could easily reintroduce it). `start` should clamp to the end of
    /// `items`, not walk past it.
    #[test]
    fn render_selected_out_of_range_does_not_panic() {
        let state = SchemaTreeState {
            items: vec![
                SchemaTreeItem {
                    name: "a".to_string(),
                    type_name: "int64".to_string(),
                    nullable: false,
                    depth: 0,
                },
                SchemaTreeItem {
                    name: "b".to_string(),
                    type_name: "int64".to_string(),
                    nullable: false,
                    depth: 0,
                },
            ],
            selected: 2, // one past the last valid index (0..=1)
        };
        let backend = TestBackend::new(20, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        let area = Rect::new(0, 0, 20, 1);

        terminal
            .draw(|frame| {
                state.render(frame, area, Block::default().borders(Borders::ALL));
            })
            .unwrap();
    }
}
