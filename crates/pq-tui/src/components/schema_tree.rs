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
    let type_name = match dt {
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
            return;
        }
        DataType::List(inner) | DataType::LargeList(inner) => {
            items.push(SchemaTreeItem {
                name: name.to_string(),
                type_name: format!("list<{:?}>", inner.data_type()),
                nullable,
                depth,
            });
            return;
        }
        other => format!("{other:?}"),
    };

    items.push(SchemaTreeItem {
        name: name.to_string(),
        type_name,
        nullable,
        depth,
    });
}
