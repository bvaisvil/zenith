/**
 * Copyright 2019-2022, Benjamin Vaisvil and the zenith contributors
 */
use super::{Render, ZBackend};
use crate::metrics::*;
use chrono::prelude::DateTime;
use chrono::Duration as CDuration;
use chrono::{Datelike, Local, Timelike};
use starship_battery::units::power::watt;
use starship_battery::units::ratio::percent;
use starship_battery::units::time::second;
use std::ops::Mul;
use std::time::Duration;
use tui::layout::Rect;
use tui::style::{Color, Modifier, Style};
use tui::text::{Span, Spans};
use tui::widgets::Paragraph;
use tui::Frame;
use unicode_width::UnicodeWidthStr;

fn display_time(start: DateTime<Local>, end: DateTime<Local>) -> String {
    if start.day() == end.day() && start.month() == end.month() {
        return format!(
            " ({:02}:{:02}:{:02} - {:02}:{:02}:{:02})",
            start.hour(),
            start.minute(),
            start.second(),
            end.hour(),
            end.minute(),
            end.second()
        );
    }
    format!(
        " ({:} {:02}:{:02}:{:02} - {:} {:02}:{:02}:{:02})",
        start.date_naive(),
        start.hour(),
        start.minute(),
        start.second(),
        end.date_naive(),
        end.hour(),
        end.minute(),
        end.second()
    )
}

fn render_battery_widget(
    batteries: &[starship_battery::Battery],
) -> (Span<'_>, Span<'_>, Span<'_>, Span<'_>) {
    let default_style = Style::default().bg(Color::DarkGray).fg(Color::White);
    if !batteries.is_empty() {
        let b: &starship_battery::Battery = batteries.get(0).expect("no battery");
        let charge_state = match b.state() {
            starship_battery::State::Unknown => " ",
            starship_battery::State::Charging => "⚡︎",
            starship_battery::State::Discharging => "🁢",
            starship_battery::State::Empty => "🁢",
            starship_battery::State::Full => "🁢",
            _ => "",
        };
        let charge_state_color = match b.state() {
            starship_battery::State::Charging => Color::Green,
            starship_battery::State::Discharging => Color::Yellow,
            starship_battery::State::Empty => Color::Red,
            starship_battery::State::Full => Color::Green,
            _ => Color::White,
        };
        let t = match b.state() {
            starship_battery::State::Charging => match b.time_to_full() {
                Some(t) => {
                    let t = CDuration::from_std(Duration::from_secs(t.get::<second>() as u64))
                        .expect("Duration out of range.");
                    format!("{:}:{:}", t.num_hours(), t.num_minutes() % 60)
                }
                None => String::from(""),
            },
            starship_battery::State::Discharging => match b.time_to_empty() {
                Some(t) => {
                    let t = CDuration::from_std(Duration::from_secs(t.get::<second>() as u64))
                        .expect("Duration out of range.");
                    format!("{:02}:{:02}", t.num_hours(), t.num_minutes() % 60)
                }
                None => String::from(""),
            },
            _ => String::from(""),
        };
        let charged = b.state_of_charge().get::<percent>();
        let charged_color = if charged > 0.75 {
            Color::Green
        } else if charged > 50.0 {
            Color::Yellow
        } else {
            Color::Red
        };
        (
            Span::styled(charge_state, default_style.fg(charge_state_color)),
            Span::styled(
                format!(" {:03.2}%", charged),
                default_style.fg(charged_color),
            ),
            Span::styled(format!(" {:}", t), default_style),
            Span::styled(
                format!(" {:03.2}w", b.energy_rate().get::<watt>()),
                default_style,
            ),
        )
    } else {
        (
            Span::styled("", default_style),
            Span::styled("", default_style),
            Span::styled("", default_style),
            Span::styled("", default_style),
        )
    }
}

pub fn render_top_title_bar(
    app: &CPUTimeApp,
    area: Rect,
    f: &mut Frame<'_, ZBackend>,
    zf: &u32,
    offset: &usize,
) {
    let tick = app.histogram_map.tick;
    let hist_duration = app.histogram_map.hist_duration(area.width as usize, *zf);
    let offset_duration = chrono::Duration::from_std(tick.mul(*offset as u32).mul(*zf))
        .expect("Couldn't convert from std");
    let uptime = match CDuration::from_std(app.uptime) {
        Ok(d) => format!(
            " [Up {:} days {:02}:{:02}:{:02}]",
            d.num_days(),
            d.num_hours() % 24,
            d.num_minutes() % 60,
            d.num_seconds() % 60
        ),
        Err(_) => String::from(""),
    };
    let now = Local::now();
    let start = now
        .checked_sub_signed(hist_duration + offset_duration)
        .expect("Couldn't compute time");
    let end = now
        .checked_sub_signed(offset_duration)
        .expect("Couldn't add time");
    let default_style = Style::default().bg(Color::DarkGray).fg(Color::White);
    let back_in_time = if offset_duration.num_seconds() > 0 {
        format!(
            "(-{:02}:{:02}:{:02})",
            offset_duration.num_hours(),
            offset_duration.num_minutes() % 60,
            offset_duration.num_seconds() % 60
        )
    } else {
        String::from("")
    };
    let battery_widets = render_battery_widget(&app.batteries);
    let battery_start = if !app.batteries.is_empty() { " [" } else { "" };
    let battery_end = if !app.batteries.is_empty() { "]" } else { "" };
    let not_recording_warning = if app.writes_db_store() {
        ""
    } else {
        " History not recording, more info: (h) "
    };

    let mut line = vec![
        Span::styled(
            format!(" {:}", app.hostname),
            default_style.add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" [{:} {:}]", app.osname, app.release),
            default_style,
        ),
        Span::styled(uptime, default_style),
        Span::styled(battery_start, default_style),
        battery_widets.0,
        battery_widets.1,
        battery_widets.2,
        battery_widets.3,
        Span::styled(battery_end, default_style),
        Span::styled(" [Showing: ", default_style),
        Span::styled(
            format!("{:} mins", hist_duration.num_minutes()),
            default_style.fg(Color::Green),
        ),
        Span::styled(display_time(start, end), default_style),
        Span::styled(back_in_time, default_style.add_modifier(Modifier::BOLD)),
        Span::styled("]", default_style),
        Span::styled(" (h)elp", default_style),
        Span::styled(" (q)uit", default_style),
        Span::styled(" sect(i)ons", default_style),
    ];

    let used_width: usize = line.iter().map(|s| s.content.width()).sum();
    line.push(Span::styled(
        format!(
            "{:>width$}",
            not_recording_warning,
            width = ((area.width as usize).saturating_sub(used_width))
        ),
        default_style.fg(Color::Red).add_modifier(Modifier::BOLD),
    ));

    Paragraph::new(Spans::from(line)).render(f, area);
}
