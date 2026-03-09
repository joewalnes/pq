use ratatui::style::Color;

pub struct Theme {
    pub header_bg: Color,
    pub header_fg: Color,
    pub selected_bg: Color,
    pub selected_fg: Color,
    pub border_active: Color,
    pub border_inactive: Color,
    pub status_fg: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            header_bg: Color::Blue,
            header_fg: Color::White,
            selected_bg: Color::DarkGray,
            selected_fg: Color::White,
            border_active: Color::Cyan,
            border_inactive: Color::DarkGray,
            status_fg: Color::Yellow,
        }
    }
}
