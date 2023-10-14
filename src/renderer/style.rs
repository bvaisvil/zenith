use ratatui::style::{Color, Modifier, Style};

pub const MAX_COLOR: Color = Color::Red;
pub const OK_COLOR: Color = Color::Green;

pub fn max_style() -> Style {
    Style::default().fg(MAX_COLOR).add_modifier(Modifier::BOLD)
}

pub fn ok_style() -> Style {
    Style::default().fg(OK_COLOR)
}
