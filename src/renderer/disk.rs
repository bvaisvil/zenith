/**
 * Copyright 2019-2022, Benjamin Vaisvil and the zenith contributors
 */
use super::{split_left_right_pane, FileSystemDisplay, Render, ZBackend};
use crate::float_to_byte_string;
use crate::metrics::histogram::{HistogramKind, View};
use crate::metrics::CPUTimeApp;
use byte_unit::{Byte, ByteUnit};
use std::borrow::Cow;
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::{Color, Modifier, Style};
use tui::text::{Span, Spans};
use tui::widgets::{Block, Borders, List, ListItem, Paragraph, Sparkline};
use tui::Frame;

pub fn render_disk(
    app: &CPUTimeApp,
    layout: Rect,
    f: &mut Frame<'_, ZBackend>,
    view: View,
    border_style: Style,
    file_system_index: &usize,
    file_system_display: &FileSystemDisplay,
) {
    let (disk_layout, view) = split_left_right_pane("Disk", layout, f, view, border_style);
    let area = Layout::default()
        .margin(1)
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(disk_layout[1]);

    if *file_system_display == FileSystemDisplay::Activity {
        disk_activity_histogram(app, f, view, &area, file_system_index);
    } else {
        disk_usage(app, f, view, &area, file_system_index);
    }
    let mut disk_list: Vec<_> = app.disks.values().collect();
    disk_list.sort_by(|a, b| b.mount_point.cmp(&a.mount_point));
    let disks: Vec<_> = disk_list
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let style = if d.get_perc_free_space() < 10.0 {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            };
            if *file_system_index == i {
                Span::styled(
                    Cow::Owned(format!(
                        "→{:3.0}%: {}",
                        d.get_perc_free_space(),
                        d.mount_point.display()
                    )),
                    style,
                )
            } else {
                Span::styled(
                    Cow::Owned(format!(
                        " {:3.0}%: {}",
                        d.get_perc_free_space(),
                        d.mount_point.display()
                    )),
                    style,
                )
            }
        })
        .map(ListItem::new)
        .collect();
    List::new(disks)
        .block(
            Block::default()
                .title(Span::styled(
                    "File Systems [(a)ctivity/usage]",
                    border_style,
                ))
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .render(f, disk_layout[0]);
}
fn disk_activity_histogram(
    app: &CPUTimeApp,
    f: &mut Frame<'_, ZBackend>,
    view: View,
    area: &[Rect],
    file_system_index: &usize,
) {
    let mut disk_list: Vec<_> = app.disks.values().collect();
    disk_list.sort_by(|a, b| b.mount_point.cmp(&a.mount_point));
    if let Some(fs) = disk_list.get(*file_system_index) {
        let read_up =
            float_to_byte_string!(fs.get_read_bytes_sec(&app.histogram_map.tick), ByteUnit::B);
        let h_read = match app
            .histogram_map
            .get_zoomed(&HistogramKind::IoRead(fs.name.to_string()), &view)
        {
            Some(h) => h,
            None => return,
        };

        let read_max: u64 = match h_read.data().iter().max() {
            Some(x) => *x,
            None => 1,
        };
        let read_max_bytes = float_to_byte_string!(read_max as f64, ByteUnit::B);

        let top_reader = match app.top_pids.read.pid {
            Some(pid) => match app.process_map.get(&pid) {
                Some(p) => format!("[{:} - {:} - {:}]", p.pid, p.name, p.user_name),
                None => String::from(""),
            },
            None => String::from(""),
        };

        let write_down =
            float_to_byte_string!(fs.get_write_bytes_sec(&app.histogram_map.tick), ByteUnit::B);
        let h_write = match app
            .histogram_map
            .get_zoomed(&HistogramKind::IoWrite(fs.name.to_string()), &view)
        {
            Some(h) => h,
            None => return,
        };

        let write_max: u64 = match h_write.data().iter().max() {
            Some(x) => *x,
            None => 1,
        };
        let write_max_bytes = float_to_byte_string!(write_max as f64, ByteUnit::B);

        let top_writer = match app.top_pids.write.pid {
            Some(pid) => match app.process_map.get(&pid) {
                Some(p) => format!("[{:} - {:} - {:}]", p.pid, p.name, p.user_name),
                None => String::from(""),
            },
            None => String::from(""),
        };

        let top_io_waiter = match app.top_pids.iowait.pid {
            Some(pid) => match app.process_map.get(&pid) {
                Some(p) => format!("IO WAIT [{:3.0}% {:} - {:} - {:}]", p.get_io_wait(&app.histogram_map.tick), p.pid, p.name, p.user_name),
                None => String::from(""),
            },
            None => String::from("")
        };

        Sparkline::default()
            .block(
                Block::default().title(
                    format!(
                        "R [{:^10}/s] MAX [{:^10}/s] TOP {:} {:}",
                        read_up, read_max_bytes, top_reader, top_io_waiter
                    )
                    .as_str(),
                ),
            )
            .data(h_read.data())
            .style(Style::default().fg(Color::LightYellow))
            .max(read_max)
            .render(f, area[0]);

        Sparkline::default()
            .block(
                Block::default().title(
                    format!(
                        "W [{:^10}/s] MAX [{:^10}/s] TOP {:}",
                        write_down, write_max_bytes, top_writer
                    )
                    .as_str(),
                ),
            )
            .data(h_write.data())
            .style(Style::default().fg(Color::LightMagenta))
            .max(write_max)
            .render(f, area[1]);
    }
}

fn disk_usage(
    app: &CPUTimeApp,
    f: &mut Frame<'_, ZBackend>,
    view: View,
    area: &[Rect],
    file_system_index: &usize,
) {
    let mut disk_list: Vec<_> = app.disks.values().collect();
    disk_list.sort_by(|a, b| b.mount_point.cmp(&a.mount_point));
    if let Some(fs) = disk_list.get(*file_system_index) {
        let h_used = match app
            .histogram_map
            .get_zoomed(&HistogramKind::FileSystemUsedSpace(fs.name.clone()), &view)
        {
            Some(h) => h,
            None => return,
        };
        let free = float_to_byte_string!(fs.available_bytes as f64, ByteUnit::B);
        let used = float_to_byte_string!(fs.get_used_bytes() as f64, ByteUnit::B);
        let size = float_to_byte_string!(fs.size_bytes as f64, ByteUnit::B);
        Sparkline::default()
            .block(
                Block::default().title(
                    format!(
                        "{}  ↓USED [{:^10} ({:.1}%)] FREE [{:^10} ({:.1}%)] SIZE [{:^10}]",
                        fs.name,
                        used,
                        fs.get_perc_used_space(),
                        free,
                        fs.get_perc_free_space(),
                        size
                    )
                    .as_str(),
                ),
            )
            .data(h_used.data())
            .style(Style::default().fg(Color::LightYellow))
            .max(fs.size_bytes)
            .render(f, area[0]);
        let columns = Layout::default()
            .margin(1)
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
            .split(area[1]);
        let rhs_style = Style::default().fg(Color::Green);
        let text = vec![
            Spans::from(vec![
                Span::raw("Device:                ".to_string()),
                Span::styled(fs.name.to_string(), rhs_style),
            ]),
            Spans::from(vec![
                Span::raw("File System:           ".to_string()),
                Span::styled(fs.file_system.to_string(), rhs_style),
            ]),
            Spans::from(vec![
                Span::raw("Mount Point:           ".to_string()),
                Span::styled(fs.mount_point.to_string_lossy(), rhs_style),
            ]),
            Spans::from(vec![
                Span::raw("Read:                  ".to_string()),
                Span::styled(
                    format!(
                        "{:} /s ({:})",
                        float_to_byte_string!(
                            fs.get_read_bytes_sec(&app.histogram_map.tick),
                            ByteUnit::B
                        ),
                        float_to_byte_string!(fs.current_io.read_bytes as f64, ByteUnit::B),
                    ),
                    rhs_style,
                ),
            ]),
        ];
        Paragraph::new(text).render(f, columns[0]);
        let text = vec![
            Spans::from(vec![
                Span::raw("Size:                  ".to_string()),
                Span::styled(size, rhs_style),
            ]),
            Spans::from(vec![
                Span::raw("Used:                  ".to_string()),
                Span::styled(used, rhs_style),
            ]),
            Spans::from(vec![
                Span::raw("Free:                  ".to_string()),
                Span::styled(free, rhs_style),
            ]),
            Spans::from(vec![
                Span::raw("Write:                 ".to_string()),
                Span::styled(
                    format!(
                        "{:} /s ({:})",
                        float_to_byte_string!(
                            fs.get_write_bytes_sec(&app.histogram_map.tick),
                            ByteUnit::B
                        ),
                        float_to_byte_string!(fs.current_io.write_bytes as f64, ByteUnit::B),
                    ),
                    rhs_style,
                ),
            ]),
        ];
        Paragraph::new(text).render(f, columns[1]);
    }
}
