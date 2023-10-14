/**
 * Copyright 2019-2022, Benjamin Vaisvil and the zenith contributors
 */
use crate::metrics::*;
use crate::renderer::{HistoryRecording, Render, ZBackend};
#[cfg(all(target_os = "linux", feature = "nvidia"))]
use nvml::error::NvmlError;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

pub fn render_help(
    _app: &CPUTimeApp,
    area: Rect,
    f: &mut Frame<'_, ZBackend>,
    history_recording: HistoryRecording,
) {
    let header_style = Style::default().fg(Color::Green);
    let main_style = Style::default();
    let key_style = main_style.fg(Color::Cyan);

    static GLOBAL_KEYS: &[[&str; 2]] = &[
        ["h    ", "    Toggle this help screen\n"],
        ["q    ", "    Quit and exit zenith\n"],
        ["<TAB>", "    Changes highlighted section\n"],
        ["e    ", "    Expands highlighted section\n"],
        ["m    ", "    Shrinks highlighted section\n"],
        ["F1   ", "    Show Section Selection Menu\n"],
        ["-    ", "    Zoom chart out\n"],
        ["+    ", "    Zoom chart in\n"],
        ["←    ", "    Move back in time\n"],
        ["→    ", "    Move forward In time\n"],
        ["`    ", "    Reset charts to current\n"],
    ];

    static PROCESS_TABLE_KEYS: &[[&str; 2]] = &[
        ["<RET> ", "    Focus current process\n"],
        ["↓     ", "    Move one line down\n"],
        ["↑     ", "    Move one line up\n"],
        ["PgDown", "    Move view one screen down\n"],
        ["PgUp  ", "    Move view one screen up\n"],
        ["Home  ", "    Move to top\n"],
        ["End   ", "    Move to bottom\n"],
        [";     ", "    Change sort between ascending/descending\n"],
        [",     ", "    Cycle columns left\n"],
        [".     ", "    Cycle columns right\n"],
        ["p     ", "    Toggle paths on/off\n"],
        ["/     ", "    Toggle filter mode\n"],
        ["<ESC> ", "    Leave filter mode\n"],
    ];

    let mut t = vec![Line::from(vec![Span::styled(
        "Primary Interface",
        header_style,
    )])];

    for [key, text] in GLOBAL_KEYS {
        t.push(Line::from(vec![
            Span::styled(*key, key_style),
            Span::styled(*text, main_style),
        ]));
    }

    t.push(Line::from(vec![Span::styled("", header_style)]));
    t.push(Line::from(vec![Span::styled(
        "Process Table\n",
        header_style,
    )]));

    for [key, text] in PROCESS_TABLE_KEYS {
        t.push(Line::from(vec![
            Span::styled(*key, key_style),
            Span::styled(*text, main_style),
        ]));
    }

    let not_recording_reason = match history_recording {
        HistoryRecording::On => None,
        HistoryRecording::UserDisabled => {
            Some("because zenith was started with the `--disable_history` flag\n")
        }
        HistoryRecording::OtherInstancePrevents => {
            Some("because another zenith instance was already running\n")
        }
    };

    if let Some(reason) = not_recording_reason {
        t.push(Line::from(vec![Span::styled("", header_style)]));
        for s in ["Recorded data is not being saved to the database\n", reason].iter() {
            t.push(Line::from(vec![Span::styled(
                *s,
                Style::default().fg(Color::Yellow),
            )]));
        }
    }

    #[cfg(all(target_os = "linux", feature = "nvidia"))]
    if let Some(ne) = &_app.nvml_error {
        let content = match ne {
            NvmlError::DriverNotLoaded => "NVIDIA Error: No Driver Detected.",
            NvmlError::NoPermission => "NVIDIA Error: Permissioned denied to talk to driver.",
            NvmlError::Unknown => "NVIDIA Error: Unkown Error.",
            _ => "",
        };
        if content.len() > 0 {
            t.push(Line::from(vec![Span::styled("", header_style)]));
            t.push(Line::from(vec![Span::styled(
                content,
                Style::default().fg(Color::Yellow),
            )]));
        }
    }

    let help_height = t.len() as u16;

    let help_layout = Layout::default()
        .horizontal_margin(5)
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(1),
                Constraint::Min(help_height),
                Constraint::Max(5),
            ]
            .as_ref(),
        )
        .split(area);
    let (title_area, help_area) = (help_layout[0], help_layout[1]);

    let b = Block::default().borders(Borders::ALL);
    Paragraph::new(t)
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Left)
        .block(b)
        .render(f, help_area);

    let t = vec![Span::styled(
        concat!("zenith v", env!("CARGO_PKG_VERSION")),
        header_style,
    )];
    Paragraph::new(Line::from(t))
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Center)
        .render(f, title_area);
}
