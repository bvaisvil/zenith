/**
 * Copyright 2019-2022, Benjamin Vaisvil and the zenith contributors
 */
use super::{split_left_right_pane, Render};
use crate::float_to_byte_string;
use crate::metrics::histogram::{HistogramKind, View};
use crate::metrics::CPUTimeApp;
use byte_unit::{Byte, Unit};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, List, ListItem, Sparkline};
use ratatui::Frame;
use std::borrow::Cow;

pub fn render_net(
    app: &CPUTimeApp,
    area: Rect,
    f: &mut Frame<'_>,
    view: View,
    border_style: Style,
) {
    let (network_layout, view) = split_left_right_pane("Network", area, f, view, border_style);
    let net = Layout::default()
        .margin(1)
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(network_layout[1]);

    let net_up = float_to_byte_string!(
        app.net_out as f64 / app.histogram_map.tick.as_secs_f64(),
        Unit::B
    );
    let h_out = match app.histogram_map.get_zoomed(&HistogramKind::NetTx, &view) {
        Some(h) => h,
        None => return,
    };

    let up_max: u64 = match h_out.data().iter().max() {
        Some(x) => *x,
        None => 1,
    };
    let up_max_bytes = float_to_byte_string!(up_max as f64, Unit::B);

    Sparkline::default()
        .block(
            Block::default()
                .title(format!("↑ [{net_up:^10}/s] PEAK [{up_max_bytes:^10}/s]").as_str()),
        )
        .data(h_out.data())
        .style(Style::default().fg(Color::LightYellow))
        .max(up_max)
        .render(f, net[0]);

    let net_down = float_to_byte_string!(
        app.net_in as f64 / app.histogram_map.tick.as_secs_f64(),
        Unit::B
    );
    let h_in = match app.histogram_map.get_zoomed(&HistogramKind::NetRx, &view) {
        Some(h) => h,
        None => return,
    };

    let down_max: u64 = match h_in.data().iter().max() {
        Some(x) => *x,
        None => 1,
    };
    let down_max_bytes = float_to_byte_string!(down_max as f64, Unit::B);
    Sparkline::default()
        .block(
            Block::default()
                .title(format!("↓ [{net_down:^10}/s] PEAK [{down_max_bytes:^10}/s]").as_str()),
        )
        .data(h_in.data())
        .style(Style::default().fg(Color::LightMagenta))
        .max(down_max)
        .render(f, net[1]);

    let ips: Vec<_> = app
        .network_interfaces
        .iter()
        .map(|n| {
            Span::styled(
                Cow::Owned(format!("{:<8.8} : {}", n.name, n.ip)),
                Style::default().fg(Color::Green),
            )
        })
        .map(ListItem::new)
        .collect();
    List::new(ips)
        .block(
            Block::default()
                .title(Span::styled("Network", border_style))
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .render(f, network_layout[0]);
}
