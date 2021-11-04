/**
 * Copyright 2019-2020, Benjamin Vaisvil and the zenith contributors
 */
use crate::histogram::{HistogramKind, View};
use crate::metrics::*;
use crate::util::*;
use crate::zprocess::*;

use battery::units::power::watt;
use battery::units::ratio::percent;
use battery::units::time::second;
use byte_unit::{Byte, ByteUnit};
use chrono::prelude::DateTime;
use chrono::Duration as CDuration;
use chrono::{Datelike, Local, Timelike};
use crossterm::{
    event::{KeyCode as Key, KeyEvent, KeyModifiers},
    execute,
    terminal::EnterAlternateScreen,
};
use num_traits::FromPrimitive;
use std::borrow::Cow;
use std::cmp::Eq;
use std::collections::HashSet;
use std::fmt;
use std::io;
use std::io::Stdout;
use std::path::PathBuf;
use std::time::{Duration, Instant, UNIX_EPOCH};
use tui::{backend::CrosstermBackend, Terminal};

use std::ops::Mul;
use tui::backend::Backend;
use tui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use tui::style::{Color, Modifier, Style};
use tui::text::{Span, Spans};
use tui::widgets::{
    BarChart, Block, Borders, List, ListItem, ListState, Paragraph, Row, Sparkline, Table, Wrap,
};
use tui::Frame;
use unicode_width::UnicodeWidthStr;

const PROCESS_SELECTION_GRACE: Duration = Duration::from_millis(2000);
const LEFT_PANE_WIDTH: u16 = 34u16;

type ZBackend = CrosstermBackend<Stdout>;

/// Compatibility trait, that preserves an older method from tui 0.6.5
/// Exists mostly to keep the caller code idiomatic for the use cases in this file
/// May be refactored out later if the widget usage patterns change
trait Render<B>
where
    B: Backend,
{
    fn render(self, f: &mut Frame<B>, area: Rect);
}

impl<T, B> Render<B> for T
where
    T: tui::widgets::Widget,
    B: Backend,
{
    fn render(self, f: &mut Frame<B>, area: Rect) {
        f.render_widget(self, area)
    }
}

macro_rules! float_to_byte_string {
    ($x:expr, $unit:expr) => {
        match Byte::from_unit($x, $unit) {
            Ok(b) => b.get_appropriate_unit(false).to_string().replace(" ", ""),
            Err(_) => String::from("Err"),
        }
    };
}

macro_rules! update_section_height {
    ($x:expr, $val:expr) => {
        if $x + $val > 0.0 && $x + $val < 100.0 {
            $x += $val;
            true
        } else {
            false
        }
    };
}

#[derive(FromPrimitive, PartialEq, Copy, Clone, Debug, Ord, PartialOrd, Eq)]
pub enum Section {
    Cpu = 0,
    Network = 1,
    Disk = 2,
    Graphics = 3,
    Process = 4,
}

#[derive(FromPrimitive, PartialEq, Copy, Clone, Debug, Ord, PartialOrd, Eq)]
enum FileSystemDisplay {
    Usage,
    Activity,
}

impl fmt::Display for Section {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let name = match self {
            Section::Cpu => " CPU",
            Section::Disk => " Disk",
            Section::Graphics => " Graphics",
            Section::Network => " Network",
            Section::Process => " Process",
        };
        write!(f, "{}", name)
    }
}

pub fn sum_section_heights(geometry: &[(Section, f64)]) -> f64 {
    let mut sum = 0.0;
    for section in geometry {
        sum += section.1;
    }
    sum
}

fn mem_title(app: &CPUTimeApp) -> String {
    let mem = percent_of(app.mem_utilization, app.mem_total) as u64;
    let swp = percent_of(app.swap_utilization, app.swap_total) as u64;

    let top_mem_proc = match app.top_mem_pid {
        Some(pid) => match app.process_map.get(&pid) {
            Some(p) => format!("[{:} - {:} - {:}]", p.pid, p.name, p.user_name),
            None => String::from(""),
        },
        None => String::from(""),
    };

    format!(
        "MEM [{} / {} - {:}%] SWP [{} / {} - {:}%] {:}",
        float_to_byte_string!(app.mem_utilization as f64, ByteUnit::KB),
        float_to_byte_string!(app.mem_total as f64, ByteUnit::KB),
        mem,
        float_to_byte_string!(app.swap_utilization as f64, ByteUnit::KB),
        float_to_byte_string!(app.swap_total as f64, ByteUnit::KB),
        swp,
        top_mem_proc
    )
}

fn cpu_title(app: &CPUTimeApp, histogram: &[u64]) -> String {
    let top_process_name = match &app.cum_cpu_process {
        Some(p) => p.name.as_str(),
        None => "",
    };
    let top_process_amt = match &app.cum_cpu_process {
        Some(p) => p.user_name.as_str(),
        None => "",
    };
    let top_pid = match &app.cum_cpu_process {
        Some(p) => p.pid,
        None => 0,
    };
    let mean: f64 = match histogram.len() {
        0 => 0.0,
        _ => histogram.iter().sum::<u64>() as f64 / histogram.len() as f64,
    };
    let temp = if !app.sensors.is_empty() {
        let t = app
            .sensors
            .iter()
            .map(|s| format!("{: >3.0}", s.current_temp))
            .collect::<Vec<String>>()
            .join(",");
        format!(" TEMP [{:}°C]", t)
    } else {
        String::from("")
    };
    format!(
        "CPU [{: >3}%]{:} MEAN [{: >3.2}%] TOP [{} - {} - {}]",
        app.cpu_utilization, temp, mean, top_pid, top_process_name, top_process_amt
    )
}

fn render_process_table(
    app: &CPUTimeApp,
    process_table: &[i32],
    area: Rect,
    process_table_start: usize,
    f: &mut Frame<'_, ZBackend>,
    border_style: Style,
    show_paths: bool,
    show_find: bool,
    filter: &str,
    highlighted_row: usize,
) -> Option<Box<ZProcess>> {
    // 4 for the margins and table header
    let display_height = match area.height.saturating_sub(4) {
        0 => return None,
        v => v as usize,
    };

    let procs: Vec<&ZProcess> = process_table
        .iter()
        .map(|pid| {
            app.process_map
                .get(pid)
                .expect("expected pid to be present")
        })
        .collect();
    let highlighted_process = if !procs.is_empty() {
        Some(Box::new(procs[highlighted_row].clone()))
    } else {
        None
    };
    if area.height < 5 {
        return highlighted_process; // not enough space to draw anything
    }
    let rows: Vec<Row> = procs
        .iter()
        .enumerate()
        .skip(process_table_start)
        .take(display_height)
        .map(|(i, p)| {
            let cmd_string = if show_paths {
                if p.command.len() > 1 {
                    format!(" - {:}", p.command.join(" "))
                } else if !p.command.is_empty() {
                    format!(" - {:}", p.command[0])
                } else {
                    String::from("")
                }
            } else if p.command.len() > 1 {
                format!(" {:}", p.command[1..].join(" "))
            } else {
                String::from("")
            };
            let mut row = vec![
                format!("{: >width$}", p.pid, width = app.max_pid_len),
                format!("{: <10}", p.user_name),
                format!("{: <3}", p.priority),
                format!("{: <3}", p.nice),
                format!("{:>5.1}", p.cpu_usage),
                format!("{:>5.1}", percent_of(p.memory, app.mem_total)),
                format!(
                    "{:>8}",
                    float_to_byte_string!(p.memory as f64, ByteUnit::KB).replace("B", "")
                ),
                format!(
                    "{: >8}",
                    float_to_byte_string!(p.virtual_memory as f64, ByteUnit::KB).replace("B", "")
                ),
                format!("{:1}", p.status.to_single_char()),
                format!(
                    "{:>8}",
                    float_to_byte_string!(
                        p.get_read_bytes_sec(&app.histogram_map.tick),
                        ByteUnit::B
                    )
                    .replace("B", "")
                ),
                format!(
                    "{:>8}",
                    float_to_byte_string!(
                        p.get_write_bytes_sec(&app.histogram_map.tick),
                        ByteUnit::B
                    )
                    .replace("B", "")
                ),
            ];
            if !app.gfx_devices.is_empty() {
                row.push(format!("{:>4.0}", p.gpu_usage));
                row.push(format!("{:>4.0}", p.fb_utilization));
            }
            row.push(format!("{:}{:}", p.name, cmd_string));

            let row = Row::new(row);

            if i == highlighted_row {
                row.style(
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                row
            }
        })
        .collect();

    let mut header = vec![
        format!("{:<width$}", "PID", width = app.max_pid_len + 1),
        String::from("USER       "),
        String::from("P   "),
        String::from("N   "),
        String::from("CPU%  "),
        String::from("MEM%  "),
        String::from("MEM     "),
        String::from("VIRT     "),
        String::from("S "),
        String::from("READ/s   "),
        String::from("WRITE/s  "),
    ];
    if !app.gfx_devices.is_empty() {
        header.push(String::from("GPU% "));
        header.push(String::from("FB%  "));
    }
    //figure column widths
    let mut widths = Vec::with_capacity(header.len() + 1);
    let mut used_width = 0;
    for item in &header {
        let len = item.len() as u16;
        widths.push(Constraint::Length(len));
        used_width += len;
    }
    let cmd_width = f.size().width.saturating_sub(used_width).saturating_sub(3);
    let cmd_header = format!("{:<width$}", "CMD", width = cmd_width as usize);
    widths.push(Constraint::Min(cmd_width));
    header.push(cmd_header);

    header[app.psortby as usize].pop();
    let sort_ind = match app.psortorder {
        ProcessTableSortOrder::Ascending => '↑',
        ProcessTableSortOrder::Descending => '↓',
    };
    header[app.psortby as usize].insert(0, sort_ind); //sort column indicator

    let title = if show_find {
        format!("[ESC] Clear, Find: {:}", filter)
    } else if !filter.is_empty() {
        format!("Filtered Results: {:}, [/] to change/clear", filter)
    } else {
        format!(
            "Tasks [{:}] Threads [{:}]  Navigate [↑/↓] Sort Col [,/.] Asc/Dec [;] Filter [/]",
            app.processes.len(),
            app.threads_total
        )
    };

    Table::new(rows)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(Span::styled(title, border_style)),
        )
        .widths(widths.as_slice())
        .column_spacing(0)
        .header(
            Row::new(header)
                .style(Style::default().bg(Color::DarkGray))
                .bottom_margin(1),
        )
        .render(f, area);
    highlighted_process
}

fn render_cpu_histogram(app: &CPUTimeApp, area: Rect, f: &mut Frame<'_, ZBackend>, view: &View) {
    let h = match app.histogram_map.get_zoomed(&HistogramKind::Cpu, view) {
        Some(h) => h,
        None => return,
    };
    let title = cpu_title(app, h.data());
    Sparkline::default()
        .block(Block::default().title(title.as_str()))
        .data(h.data())
        .style(Style::default().fg(Color::Blue))
        .max(100)
        .render(f, area);
}

fn render_memory_histogram(app: &CPUTimeApp, area: Rect, f: &mut Frame<'_, ZBackend>, view: &View) {
    let h = match app.histogram_map.get_zoomed(&HistogramKind::Mem, view) {
        Some(h) => h,
        None => return,
    };
    let title2 = mem_title(app);
    Sparkline::default()
        .block(Block::default().title(title2.as_str()))
        .data(h.data())
        .style(Style::default().fg(Color::Cyan))
        .max(100)
        .render(f, area);
}

fn render_cpu_bars(app: &CPUTimeApp, area: Rect, f: &mut Frame<'_, ZBackend>, style: &Style) {
    let cpus = app.cpus.to_owned();
    if cpus.is_empty() {
        return;
    }

    let core_count = cpus.len() as u16;
    let widest_label = cpus.iter().map(|(s, _)| s.len()).max().unwrap_or(0) as u16;
    let full_width = widest_label * core_count + (core_count - 1);

    Block::default()
        .title(Span::styled(
            format!(
                "CPU{} {}@{} MHz",
                if core_count > 1 { "S" } else { "" },
                core_count,
                app.frequency
            ),
            *style,
        ))
        .borders(Borders::ALL)
        .border_style(*style)
        .render(f, area);

    let layout = Layout::default().margin(1).direction(Direction::Vertical);

    if full_width > 2 * area.width {
        // won't fit in 2 rows of bars, using grid layout

        let layout = layout
            .constraints(vec![Constraint::Percentage(100)])
            .split(area);

        let cols = 4;

        let nrows = ((cpus.len() as u16 + cols - 1) / cols) as usize;

        let mut items = vec![];
        for i in 0..nrows {
            cpus.iter()
                .skip(i)
                .step_by(nrows)
                .take(cols.into())
                .for_each(|(label, load)| {
                    items.push(Span::raw(format!("{:<2} ", label)));
                    let color = if *load < 90 { Color::Green } else { Color::Red };
                    items.push(Span::styled(
                        format!("{:3}", load),
                        Style::default().fg(color),
                    ));
                    items.push(Span::raw("% "));
                });
        }

        Paragraph::new(Spans::from(items))
            .wrap(Wrap { trim: false })
            .render(f, layout[0]);

        return;
    }

    // displaying as bars
    let bars: Vec<(&str, u64)> = cpus.iter().map(|(p, u)| (p.as_str(), *u)).collect();

    fn clamp_up(val: u16, upper: u16) -> u16 {
        if val > upper {
            upper
        } else {
            val
        }
    }
    let max_bar_width = 3;

    fn styled_bar_chart() -> BarChart<'static> {
        BarChart::default()
            .bar_gap(1)
            .max(100)
            .bar_style(Style::default().fg(Color::Green))
            .value_style(
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
    }

    if full_width <= area.width {
        // fits in one row

        let cpu_bar_layout = layout
            .constraints(vec![Constraint::Percentage(100)])
            .split(area);

        let bar_width = clamp_up((area.width - (core_count - 1)) / core_count, max_bar_width);

        styled_bar_chart()
            .data(bars.as_slice())
            .bar_width(bar_width)
            .render(f, cpu_bar_layout[0]);
    } else {
        // fits on two rows

        let half = bars.len() / 2;

        let cpu_bar_layout = layout
            .constraints(vec![Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        let bar_width = clamp_up(
            (area.width * 2 - (core_count - 1)) / core_count,
            max_bar_width,
        );

        styled_bar_chart()
            .data(&bars[half..])
            .bar_width(bar_width)
            .render(f, cpu_bar_layout[1]);

        styled_bar_chart()
            .data(&bars[0..half])
            .bar_width(bar_width)
            .render(f, cpu_bar_layout[0]);
    }
}

fn render_net(
    app: &CPUTimeApp,
    area: Rect,
    f: &mut Frame<'_, ZBackend>,
    view: View,
    border_style: Style,
) {
    Block::default()
        .title("Network")
        .borders(Borders::ALL)
        .border_style(border_style)
        .render(f, area);
    let network_layout = Layout::default()
        .margin(0)
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(LEFT_PANE_WIDTH), Constraint::Min(10)].as_ref())
        .split(area);
    let net = Layout::default()
        .margin(1)
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(network_layout[1]);

    let view = View {
        width: net[0].width as usize,
        ..view
    };

    let net_up = float_to_byte_string!(
        app.net_out as f64 / app.histogram_map.tick.as_secs_f64(),
        ByteUnit::B
    );
    let h_out = match app.histogram_map.get_zoomed(&HistogramKind::NetTx, &view) {
        Some(h) => h,
        None => return,
    };

    let up_max: u64 = match h_out.data().iter().max() {
        Some(x) => *x,
        None => 1,
    };
    let up_max_bytes = float_to_byte_string!(up_max as f64, ByteUnit::B);

    Sparkline::default()
        .block(
            Block::default()
                .title(format!("↑ [{:^10}/s] Max [{:^10}/s]", net_up, up_max_bytes).as_str()),
        )
        .data(h_out.data())
        .style(Style::default().fg(Color::LightYellow))
        .max(up_max)
        .render(f, net[0]);

    let net_down = float_to_byte_string!(
        app.net_in as f64 / app.histogram_map.tick.as_secs_f64(),
        ByteUnit::B
    );
    let h_in = match app.histogram_map.get_zoomed(&HistogramKind::NetRx, &view) {
        Some(h) => h,
        None => return,
    };

    let down_max: u64 = match h_in.data().iter().max() {
        Some(x) => *x,
        None => 1,
    };
    let down_max_bytes = float_to_byte_string!(down_max as f64, ByteUnit::B);
    Sparkline::default()
        .block(
            Block::default()
                .title(format!("↓ [{:^10}/s] Max [{:^10}/s]", net_down, down_max_bytes).as_str()),
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

fn render_process(
    app: &CPUTimeApp,
    layout: Rect,
    f: &mut Frame<'_, ZBackend>,
    border_style: Style,
    process_message: &Option<String>,
    p: &ZProcess,
) {
    Block::default()
        .title(Span::styled(format!("Process: {0}", p.name), border_style))
        .borders(Borders::ALL)
        .border_style(border_style)
        .render(f, layout);
    let v_sections = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Length(2), Constraint::Min(1)].as_ref())
        .split(layout);

    let title = format!("(b)ack (n)ice (p)riority 0 (s)uspend (r)esume (k)ill [SIGKILL] (t)erminate [SIGTERM] {:} {: >width$}", 
                        process_message.as_ref().unwrap_or(&String::from("")), "", width = layout.width as usize);

    Block::default()
        .title(Span::styled(
            title,
            Style::default().bg(Color::DarkGray).fg(Color::White),
        ))
        .render(f, v_sections[0]);

    //Block::default().borders(Borders::LEFT).render(f, h_sections[1]);

    let alive = if p.end_time.is_some() {
        format!(
            "dead since {:}",
            DateTime::<Local>::from(UNIX_EPOCH + Duration::from_secs(p.end_time.unwrap()))
        )
    } else {
        "alive".to_string()
    };
    let start_time = DateTime::<Local>::from(UNIX_EPOCH + Duration::from_secs(p.start_time));
    let et = match p.end_time {
        Some(t) => DateTime::<Local>::from(UNIX_EPOCH + Duration::from_secs(t)),
        None => Local::now(),
    };
    let d = et - start_time;
    let d = format!(
        "{:0>2}:{:0>2}:{:0>2}",
        d.num_hours(),
        d.num_minutes() % 60,
        d.num_seconds() % 60
    );

    let rhs_style = Style::default().fg(Color::Green);
    let mut text = vec![
        Spans::from(vec![
            Span::raw("Name:                  "),
            Span::styled(format!("{:} ({:})", &p.name, alive), rhs_style),
        ]),
        Spans::from(vec![
            Span::raw("PID:                   "),
            Span::styled(
                format!("{:>width$}", &p.pid, width = app.max_pid_len),
                rhs_style,
            ),
        ]),
        Spans::from(vec![
            Span::raw("Command:               "),
            Span::styled(p.command.join(" "), rhs_style),
        ]),
        Spans::from(vec![
            Span::raw("User:                  "),
            Span::styled(&p.user_name, rhs_style),
        ]),
        Spans::from(vec![
            Span::raw("Start Time:            "),
            Span::styled(format!("{:}", start_time), rhs_style),
        ]),
        Spans::from(vec![
            Span::raw("Total Run Time:        "),
            Span::styled(d, rhs_style),
        ]),
        Spans::from(vec![
            Span::raw("CPU Usage:             "),
            Span::styled(format!("{:>7.2} %", &p.cpu_usage), rhs_style),
        ]),
        Spans::from(vec![
            Span::raw("Threads:               "),
            Span::styled(format!("{:>7}", &p.threads_total), rhs_style),
        ]),
        Spans::from(vec![
            Span::raw("Status:                "),
            Span::styled(format!("{:}", p.status), rhs_style),
        ]),
        Spans::from(vec![
            Span::raw("Priority:              "),
            Span::styled(format!("{:>7}", p.priority), rhs_style),
        ]),
        Spans::from(vec![
            Span::raw("Nice:                  "),
            Span::styled(format!("{:>7}", p.nice), rhs_style),
        ]),
        Spans::from(vec![
            Span::raw("MEM Usage:             "),
            Span::styled(
                format!("{:>7.2} %", percent_of(p.memory, app.mem_total)),
                rhs_style,
            ),
        ]),
        Spans::from(vec![
            Span::raw("Total Memory:          "),
            Span::styled(
                format!(
                    "{:>10}",
                    float_to_byte_string!(p.memory as f64, ByteUnit::KB)
                ),
                rhs_style,
            ),
        ]),
        Spans::from(vec![
            Span::raw("Disk Read:             "),
            Span::styled(
                format!(
                    "{:>10} {:}/s",
                    float_to_byte_string!(p.read_bytes as f64, ByteUnit::B),
                    float_to_byte_string!(
                        p.get_read_bytes_sec(&app.histogram_map.tick),
                        ByteUnit::B
                    )
                ),
                rhs_style,
            ),
        ]),
        Spans::from(vec![
            Span::raw("Disk Write:            "),
            Span::styled(
                format!(
                    "{:>10} {:}/s",
                    float_to_byte_string!(p.write_bytes as f64, ByteUnit::B),
                    float_to_byte_string!(
                        p.get_write_bytes_sec(&app.histogram_map.tick),
                        ByteUnit::B
                    )
                ),
                rhs_style,
            ),
        ]),
    ];

    if !app.gfx_devices.is_empty() {
        text.push(Spans::from(vec![
            Span::raw("SM Util:            "),
            Span::styled(format!("{:7.2} %", p.sm_utilization as f64), rhs_style),
        ]));
        text.push(Spans::from(vec![
            Span::raw("Frame Buffer:       "),
            Span::styled(format!("{:7.2} %", p.fb_utilization as f64), rhs_style),
        ]));
        text.push(Spans::from(vec![
            Span::raw("Encoder Util:       "),
            Span::styled(format!("{:7.2} %", p.enc_utilization as f64), rhs_style),
        ]));
        text.push(Spans::from(vec![
            Span::raw("Decoder Util:       "),
            Span::styled(format!("{:7.2} %", p.dec_utilization as f64), rhs_style),
        ]));
    }

    if text.len() > v_sections[1].height as usize * 3 {
        let h_sections = Layout::default()
            .direction(Direction::Horizontal)
            .margin(0)
            .constraints(
                [
                    Constraint::Percentage(50),
                    Constraint::Length(1),
                    Constraint::Percentage(50),
                ]
                .as_ref(),
            )
            .split(v_sections[1]);

        let second_part = text.split_off(h_sections[0].height as usize * 3);
        Paragraph::new(text)
            .block(Block::default())
            .wrap(Wrap { trim: false })
            .render(f, h_sections[0]);

        Paragraph::new(second_part)
            .block(Block::default())
            .wrap(Wrap { trim: false })
            .render(f, h_sections[2]);
    } else {
        Paragraph::new(text)
            .block(Block::default())
            .wrap(Wrap { trim: true })
            .render(f, v_sections[1]);
    }
}

fn disk_activity_histogram(
    app: &CPUTimeApp,
    f: &mut Frame<'_, ZBackend>,
    view: View,
    area: &[Rect],
) {
    let read_up = float_to_byte_string!(app.disk_read as f64, ByteUnit::B);
    let h_read = match app.histogram_map.get_zoomed(&HistogramKind::IoRead, &view) {
        Some(h) => h,
        None => return,
    };

    let read_max: u64 = match h_read.data().iter().max() {
        Some(x) => *x,
        None => 1,
    };
    let read_max_bytes = float_to_byte_string!(read_max as f64, ByteUnit::B);

    let top_reader = match app.top_disk_reader_pid {
        Some(pid) => match app.process_map.get(&pid) {
            Some(p) => format!("[{:} - {:} - {:}]", p.pid, p.name, p.user_name),
            None => String::from(""),
        },
        None => String::from(""),
    };

    let write_down = float_to_byte_string!(app.disk_write as f64, ByteUnit::B);
    let h_write = match app.histogram_map.get_zoomed(&HistogramKind::IoWrite, &view) {
        Some(h) => h,
        None => return,
    };

    let write_max: u64 = match h_write.data().iter().max() {
        Some(x) => *x,
        None => 1,
    };
    let write_max_bytes = float_to_byte_string!(write_max as f64, ByteUnit::B);

    let top_writer = match app.top_disk_writer_pid {
        Some(pid) => match app.process_map.get(&pid) {
            Some(p) => format!("[{:} - {:} - {:}]", p.pid, p.name, p.user_name),
            None => String::from(""),
        },
        None => String::from(""),
    };
    Sparkline::default()
        .block(
            Block::default().title(
                format!(
                    "R [{:^10}/s] Max [{:^10}/s] {:}",
                    read_up, read_max_bytes, top_reader
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
                    "W [{:^10}/s] Max [{:^10}/s] {:}",
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

fn disk_usage(
    app: &CPUTimeApp,
    f: &mut Frame<'_, ZBackend>,
    view: View,
    area: &[Rect],
    file_system_index: &usize,
) {
    if let Some(fs) = app.disks.get(*file_system_index) {
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
                        "{}  ↓Used [{:^10} ({:.1}%)] Free [{:^10} ({:.1}%)] Size [{:^10}]",
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
                Span::raw("Name:                  ".to_string()),
                Span::styled(fs.name.to_string(), rhs_style),
            ]),
            Spans::from(vec![
                Span::raw("File System            ".to_string()),
                Span::styled(fs.file_system.to_string(), rhs_style),
            ]),
            Spans::from(vec![
                Span::raw("Mount Point:           ".to_string()),
                Span::styled(fs.mount_point.to_string_lossy(), rhs_style),
            ]),
        ];
        Paragraph::new(text).render(f, columns[0]);
        let text = vec![
            Spans::from(vec![
                Span::raw("Size:                  ".to_string()),
                Span::styled(size, rhs_style),
            ]),
            Spans::from(vec![
                Span::raw("Used                   ".to_string()),
                Span::styled(used, rhs_style),
            ]),
            Spans::from(vec![
                Span::raw("Free:                  ".to_string()),
                Span::styled(free, rhs_style),
            ]),
        ];
        Paragraph::new(text).render(f, columns[1]);
    }
}

fn render_disk(
    app: &CPUTimeApp,
    layout: Rect,
    f: &mut Frame<'_, ZBackend>,
    view: View,
    border_style: Style,
    file_system_index: &usize,
    file_system_display: &FileSystemDisplay,
) {
    Block::default()
        .title("Disk")
        .borders(Borders::ALL)
        .border_style(border_style)
        .render(f, layout);
    let disk_layout = Layout::default()
        .margin(0)
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(LEFT_PANE_WIDTH), Constraint::Min(10)].as_ref())
        .split(layout);
    let area = Layout::default()
        .margin(1)
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(disk_layout[1]);

    let view = View {
        width: area[0].width as usize,
        ..view
    };

    if *file_system_display == FileSystemDisplay::Activity {
        disk_activity_histogram(app, f, view, &area);
    } else {
        disk_usage(app, f, view, &area, file_system_index);
    }

    let disks: Vec<_> = app
        .disks
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

fn render_graphics(
    app: &CPUTimeApp,
    layout: Rect,
    f: &mut Frame<'_, ZBackend>,
    view: View,
    gfx_device_index: &usize,
    border_style: Style,
) {
    Block::default()
        .title("Graphics")
        .borders(Borders::ALL)
        .border_style(border_style)
        .render(f, layout);
    let gfx_layout = Layout::default()
        .margin(0)
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(LEFT_PANE_WIDTH), Constraint::Min(10)].as_ref())
        .split(layout);
    let area = Layout::default()
        .margin(1)
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(gfx_layout[1]);
    if app.gfx_devices.is_empty() {
        return;
    }

    let view = View {
        width: area[0].width as usize,
        ..view
    };

    let gd = &app.gfx_devices[*gfx_device_index];
    let h_gpu = match app
        .histogram_map
        .get_zoomed(&HistogramKind::GpuUse(gd.uuid.clone()), &view)
    {
        Some(h) => h,
        None => return,
    };
    let fan = if !gd.fans.is_empty() {
        format!("Fan [{:3.0}%]", gd.fans[0])
    } else {
        String::from("")
    };
    Sparkline::default()
        .block(
            Block::default().title(
                format!(
                    "GPU [{:3.0}%] Enc [{:3.0}%] Dec [{:3.0}%] Proc [{:}] Clock [{:}/{:} Mhz]",
                    gd.gpu_utilization,
                    gd.encoder_utilization,
                    gd.decoder_utilization,
                    gd.processes.len(),
                    gd.clock,
                    gd.max_clock
                )
                .as_str(),
            ),
        )
        .data(h_gpu.data())
        .style(Style::default().fg(Color::LightYellow))
        .max(100)
        .render(f, area[0]);

    let h_mem = match app
        .histogram_map
        .get_zoomed(&HistogramKind::GpuMem(gd.uuid.clone()), &view)
    {
        Some(h) => h,
        None => return,
    };

    Sparkline::default()
        .block(
            Block::default().title(
                format!(
                    "FB [{:3.0}%] MEM [{:} / {:} - {:}%] {:} Pwr [{:} W / {:} W] Tmp [{:} C / {:} C]",
                    gd.mem_utilization,
                    float_to_byte_string!(gd.used_memory as f64, ByteUnit::B),
                    float_to_byte_string!(gd.total_memory as f64, ByteUnit::B),
                    percent_of(gd.used_memory, gd.total_memory) as u64,
                    fan,
                    gd.power_usage / 1000,
                    gd.max_power / 1000,
                    gd.temperature,
                    gd.temperature_max
                )
                .as_str(),
            ),
        )
        .data(h_mem.data())
        .style(Style::default().fg(Color::LightMagenta))
        .max(100)
        .render(f, area[1]);
    let devices: Vec<_> = app
        .gfx_devices
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let indicator = if i == *gfx_device_index { ">" } else { " " };
            let style = if d.gpu_utilization > 90 {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            };
            Span::styled(
                Cow::Owned(format!(
                    "{}{:3.0}%: {}",
                    indicator, d.gpu_utilization, d.name
                )),
                style,
            )
        })
        .map(ListItem::new)
        .collect();
    List::new(devices)
        .block(
            Block::default()
                .title(Span::styled("Graphics Devices", border_style))
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .render(f, gfx_layout[0]);
}

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
        start.date(),
        start.hour(),
        start.minute(),
        start.second(),
        end.date(),
        end.hour(),
        end.minute(),
        end.second()
    )
}

fn render_battery_widget(
    batteries: &[battery::Battery],
) -> (Span<'_>, Span<'_>, Span<'_>, Span<'_>) {
    let default_style = Style::default().bg(Color::DarkGray).fg(Color::White);
    if !batteries.is_empty() {
        let b: &battery::Battery = batteries.get(0).expect("no battery");
        let charge_state = match b.state() {
            battery::State::Unknown => " ",
            battery::State::Charging => "⚡︎",
            battery::State::Discharging => "🁢",
            battery::State::Empty => "🁢",
            battery::State::Full => "🁢",
            _ => "",
        };
        let charge_state_color = match b.state() {
            battery::State::Charging => Color::Green,
            battery::State::Discharging => Color::Yellow,
            battery::State::Empty => Color::Red,
            battery::State::Full => Color::Green,
            _ => Color::White,
        };
        let t = match b.state() {
            battery::State::Charging => match b.time_to_full() {
                Some(t) => {
                    let t = CDuration::from_std(Duration::from_secs(t.get::<second>() as u64))
                        .expect("Duration out of range.");
                    format!("{:}:{:}", t.num_hours(), t.num_minutes() % 60)
                }
                None => String::from(""),
            },
            battery::State::Discharging => match b.time_to_empty() {
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

fn render_top_title_bar(
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
        "History not recording, more info: (h) "
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

fn render_cpu(
    app: &CPUTimeApp,
    area: Rect,
    f: &mut Frame<'_, ZBackend>,
    view: View,
    border_style: Style,
) {
    Block::default()
        .title("")
        .borders(Borders::ALL)
        .border_style(border_style)
        .render(f, area);
    let cpu_layout = Layout::default()
        .margin(0)
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(LEFT_PANE_WIDTH), Constraint::Min(10)].as_ref())
        .split(area);

    let view = View {
        width: cpu_layout[1].width as usize,
        ..view
    };

    let cpu_mem = Layout::default()
        .margin(1)
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(cpu_layout[1]);
    render_cpu_histogram(app, cpu_mem[0], f, &view);
    render_memory_histogram(app, cpu_mem[1], f, &view);
    render_cpu_bars(app, cpu_layout[0], f, &border_style);
}

fn filter_process_table<'a>(app: &'a CPUTimeApp, filter: &str) -> Cow<'a, [i32]> {
    if filter.is_empty() {
        return Cow::Borrowed(&app.processes);
    }

    let filter_lc = filter.to_lowercase();
    let results: Vec<i32> = app
        .processes
        .iter()
        .filter(|pid| {
            let p = app
                .process_map
                .get(pid)
                .expect("Pid present in processes but not in map.");
            p.name.to_lowercase().contains(&filter_lc)
                || p.exe.to_lowercase().contains(&filter_lc)
                || p.command.join(" ").to_lowercase().contains(&filter_lc)
                || format!("{:}", p.pid).contains(&filter_lc)
        })
        .copied()
        .collect();
    results.into()
}

struct SectionMGRList<'a> {
    items: Vec<(Section, ListItem<'a>)>,
    state: ListState,
}

impl<'a> SectionMGRList<'a> {
    pub fn with_geometry(geometry: Vec<(Section, f64)>) -> SectionMGRList<'a> {
        info!("Geometry: {:?}", geometry);
        info!("Geometry Len: {:?}", geometry.len());
        let mut section_set = HashSet::new();

        for (s, _) in geometry {
            section_set.insert(format!("{}", s));
        }

        debug!("Section Set: {:?}", section_set.len());
        debug!("Section Set: {:?}", section_set);
        let mut state = ListState::default();
        let items: Vec<(Section, ListItem)> = [0, 1, 2, 3, 4]
            .iter()
            .map(|i| {
                let section: Section = FromPrimitive::from_u32(*i as u32)
                    .expect("Index not in range for Section enum");
                let s: String = format!("{}", section);
                if section_set.contains(s.as_str()) {
                    (
                        section,
                        Span::styled(
                            format!("*{}", s),
                            Style::default().add_modifier(Modifier::BOLD),
                        ),
                    )
                } else {
                    (section, Span::styled(format!(" {}", s), Style::default()))
                }
            })
            .map(|(s, span)| (s, ListItem::new(span)))
            .collect();
        state.select(Some(0));
        SectionMGRList { items, state }
    }

    pub fn selected(&self) -> Option<Section> {
        self.state.selected().map(|s| self.items[s].0)
    }
}

fn render_section_mgr(list: &mut SectionMGRList<'_>, area: Rect, f: &mut Frame<'_, ZBackend>) {
    debug!("Rendering Section Manager");

    let layout = Layout::default()
        .margin(5)
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(1),
                Constraint::Percentage(80),
                Constraint::Length(5),
            ]
            .as_ref(),
        )
        .split(area);
    let header_style = Style::default().fg(Color::Green);
    let t = vec![Span::styled("Options", header_style)];
    let help = vec![Span::styled(
        "Navigate [↑/↓] Toggle [Space] Return [F1]",
        header_style,
    )];
    Paragraph::new(Spans::from(t))
        .wrap(Wrap { trim: false })
        .alignment(Alignment::Center)
        .render(f, layout[0]);
    Paragraph::new(Spans::from(help))
        .wrap(Wrap { trim: false })
        .alignment(Alignment::Center)
        .render(f, layout[2]);
    let list_items: Vec<ListItem> = list.items.iter().map(|i| i.1.clone()).collect();
    let list_widget = List::new(list_items)
        .block(Block::default().title("Sections").borders(Borders::ALL))
        .highlight_style(Style::default().bg(Color::Green))
        .highlight_symbol("➡ ");
    f.render_stateful_widget(list_widget, layout[1], &mut list.state);
}

fn render_help(area: Rect, f: &mut Frame<'_, ZBackend>, history_recording: HistoryRecording) {
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

    let mut t = vec![Spans::from(vec![Span::styled(
        "Primary Interface",
        header_style,
    )])];

    for [key, text] in GLOBAL_KEYS {
        t.push(Spans::from(vec![
            Span::styled(*key, key_style),
            Span::styled(*text, main_style),
        ]));
    }

    t.push(Spans::from(vec![Span::styled("", header_style)]));
    t.push(Spans::from(vec![Span::styled(
        "Process Table\n",
        header_style,
    )]));

    for [key, text] in PROCESS_TABLE_KEYS {
        t.push(Spans::from(vec![
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
        t.push(Spans::from(vec![Span::styled("", header_style)]));
        for s in ["Recorded data is not being saved to the database\n", reason].iter() {
            t.push(Spans::from(vec![Span::styled(
                *s,
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
    Paragraph::new(Spans::from(t))
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Center)
        .render(f, title_area);
}

/// current size of the terminal returned as (columns, rows)
fn terminal_size() -> (u16, u16) {
    crossterm::terminal::size().expect("Failed to get terminal size")
}

/// ceil to nearest upper even number
macro_rules! ceil_even {
    ($x:expr) => {
        ($x + 1) / 2 * 2
    };
}

/// Convert percentage heights to length constraints. This is done since sections other
/// than process have two sub-parts and should be of even height.
fn eval_constraints(
    section_geometry: &[(Section, f64)],
    height: u16,
    borrowed: &mut bool,
) -> Vec<Constraint> {
    debug!("Get Constraints");
    let mut constraints = vec![Constraint::Length(1)];
    let avail_height = height as i32 - 1;
    let mut process_index = -1;
    let mut max_others = 0;
    let mut max_others_index = -1;
    let mut sum_others = 0;
    // each section should have a height of at least 2 rows
    let mut max_section_height = avail_height - section_geometry.len() as i32 * 2;
    // process section is at least 4 rows high
    if section_geometry.iter().any(|s| s.0 == Section::Process) {
        max_section_height -= 2;
    }
    // convert percentage heights to length constraints and apply additional
    // criteria that height should be even number for non-process sections
    for (section_index, section) in section_geometry.iter().enumerate() {
        let required_height = section.1 * avail_height as f64 / 100.0;
        // ensure max_section_height is at least 2 after every recalculation
        max_section_height = max_section_height.max(2);
        if section.0 == Section::Process {
            process_index = section_index as i32;
            constraints.push(Constraint::Min(4));
        } else {
            // round to nearest even size for the two sub-parts in each section display
            let section_height =
                max_section_height.min(ceil_even!(required_height.floor().max(1.0) as i32));
            sum_others += section_height;
            // adjust max_section_height for subsequent sections
            max_section_height -= section_height - 2;
            if section_height >= max_others {
                max_others = section_height;
                max_others_index = section_index as i32;
            }
            constraints.push(Constraint::Length(section_height as u16));
        }
    }
    // remaining is what will be actually used for process section but if its too small (due to
    // rounding to even heights for other sections), then borrow rows from the largest section
    if process_index != -1 {
        let process_height = avail_height - sum_others;
        if process_height < 4 && max_others > 4 {
            let borrow = ceil_even!(4 - process_height).min(max_others - 4);
            // (max_others - borrow) will be >= 4 due to the min() above so cast to u16 is safe
            constraints[max_others_index as usize + 1] =
                Constraint::Length((max_others - borrow) as u16);
            constraints[process_index as usize + 1] =
                Constraint::Min((process_height + borrow) as u16);
            *borrowed = true;
        } else {
            constraints[process_index as usize + 1] = Constraint::Min(process_height as u16);
        }
    }

    constraints
}

fn get_constraints(section_geometry: &[(Section, f64)], height: u16) -> Vec<Constraint> {
    let mut borrowed = false;
    eval_constraints(section_geometry, height, &mut borrowed)
}

pub struct TerminalRenderer<'a> {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    app: CPUTimeApp,
    events: Events,
    process_table_row_start: usize,
    gfx_device_index: usize,
    file_system_index: usize,
    file_system_display: FileSystemDisplay,
    /// Index in the vector below is "order" on the screen starting from the top
    /// (usually CPU) while value is the section it belongs to and its current height (as %).
    /// Currently all sections are stacked on top of one another horizontally and
    /// occupy entire width of the screen but this may change going forward. For the case
    /// where there are multiple sections stacked vertically, the "order" can have the
    /// convention of top-bottom and left-right in each horizontal layer and the width of
    /// each section be tracked below. For more generic positioning (e.g. sections cutting
    /// across others vertically), this mapping needs to also include the position of
    /// top-left corner of the section. In that case the only significance that the
    /// "order" will have is the sequence in which the TAB key will shift focus
    /// among the sections.
    section_geometry: Vec<(Section, f64)>,
    zoom_factor: u32,
    update_number: u32,
    hist_start_offset: usize,
    selected_section_index: usize,
    constraints: Vec<Constraint>,
    process_message: Option<String>,
    show_help: bool,
    show_paths: bool,
    show_find: bool,
    show_section_mgr: bool,
    filter: String,
    highlighted_row: usize,
    selection_grace_start: Option<Instant>,
    section_manager_options: SectionMGRList<'a>,
    disable_history: bool,
}

impl<'a> TerminalRenderer<'_> {
    pub fn new(
        tick_rate: u64,
        section_geometry: &[(Section, f64)],
        db_path: Option<PathBuf>,
        disable_history: bool,
    ) -> TerminalRenderer {
        debug!("Create Metrics App");
        let app = CPUTimeApp::new(Duration::from_millis(tick_rate), db_path);
        debug!("Create Event Loop");
        let events = Events::new(app.histogram_map.tick);

        debug!("Hide Cursor");
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen).expect("Unable to enter alternate screen");
        let backend = CrosstermBackend::new(stdout);
        let mut terminal =
            Terminal::new(backend).expect("Couldn't create new terminal with backend");
        terminal.hide_cursor().ok();

        let constraints = get_constraints(section_geometry, terminal_size().1);
        let section_geometry = section_geometry.to_vec();
        TerminalRenderer {
            terminal,
            app,
            events,
            process_table_row_start: 0,
            gfx_device_index: 0,
            file_system_index: 0,
            file_system_display: FileSystemDisplay::Activity,
            section_geometry: section_geometry.clone(),
            zoom_factor: 1,
            update_number: 0,
            // select the last section by default (normally should be Process)
            selected_section_index: section_geometry.len() - 1,
            constraints,
            process_message: None,
            hist_start_offset: 0,
            show_help: false,
            show_paths: false,
            show_find: false,
            show_section_mgr: false,
            filter: String::from(""),
            highlighted_row: 0,
            selection_grace_start: None,
            section_manager_options: SectionMGRList::with_geometry(section_geometry),
            disable_history,
        }
    }

    /// Update section height by given delta value in number of rows.
    /// This transforms the value in terms of percentage and reduces the
    /// other section percentages proportionally. By this it means that
    /// larger sections will be reduced more while smaller ones will be
    /// reduced less. Overall the total percentage heights in section_geometry
    /// should always be close to 100%.
    async fn update_section_height(&mut self, delta: i16) {
        // convert val to percentage
        let (_, height) = terminal_size();
        let avail_height = (height - 1) as f64;
        let mut val = delta as f64 * 100.0 / avail_height;
        let selected_index = self.selected_section_index;
        let mut new_geometry = self.section_geometry.to_vec();
        if update_section_height!(new_geometry[selected_index].1, val) {
            // reduce proportionately from other sections if the value was updated
            let rest = 100.0 - new_geometry[selected_index].1 + val;
            for (section_index, section) in new_geometry.iter_mut().enumerate() {
                if section_index != selected_index {
                    let change = section.1 * val / rest;
                    // abort if limits are exceeded
                    if !update_section_height!(section.1, -change) {
                        val = 0.0; // abort changes
                        break;
                    }
                }
            }
            if val != 0.0 {
                let mut borrowed = false;
                let new_constraints = eval_constraints(&new_geometry, height, &mut borrowed);
                // abort if process section became too small and borrowed from others
                if !borrowed {
                    let new_sum_heights = sum_section_heights(&new_geometry);
                    assert!((99.9..=100.1).contains(&new_sum_heights));
                    self.section_geometry = new_geometry;
                    self.constraints = new_constraints;
                }
            }
        }
    }

    fn selected_section(&self) -> Section {
        self.section_geometry[self.selected_section_index].0
    }

    pub async fn start(&mut self) {
        debug!("Starting Main Loop.");
        let disable_history = self.disable_history;
        loop {
            let app = &self.app;
            let pst = &self.process_table_row_start;
            let mut width: u16 = 0;
            let mut process_table_height: u16 = 0;
            let zf = &self.zoom_factor;
            let constraints = &self.constraints;
            let geometry = &self.section_geometry.to_vec();
            let section_manager_options = &mut self.section_manager_options;
            let selected = self.section_geometry[self.selected_section_index].0;
            let process_message = &self.process_message;
            let offset = &self.hist_start_offset;
            let un = &self.update_number;
            let show_help = self.show_help;
            let show_section_mgr = self.show_section_mgr;
            let show_paths = self.show_paths;
            let filter = &self.filter;
            let show_find = self.show_find;
            let mut highlighted_process: Option<Box<ZProcess>> = None;
            let process_table = filter_process_table(app, &self.filter);
            let gfx_device_index = &self.gfx_device_index;
            let file_system_index = &self.file_system_index;
            let file_system_display = &self.file_system_display;

            if !process_table.is_empty() && self.highlighted_row >= process_table.len() {
                self.highlighted_row = process_table.len() - 1;
            }
            let highlighted_row = self.highlighted_row;

            self.terminal
                .draw(|f| {
                    width = f.size().width;
                    if show_help {
                        let v_sections = Layout::default()
                            .direction(Direction::Vertical)
                            .margin(0)
                            .constraints([Constraint::Length(1), Constraint::Length(40)].as_ref())
                            .split(f.size());

                        render_top_title_bar(app, v_sections[0], f, zf, offset);
                        let history_recording = match (app.writes_db_store(), disable_history) {
                            (true, _) => HistoryRecording::On,
                            (false, true) => HistoryRecording::UserDisabled,
                            (false, false) => HistoryRecording::OtherInstancePrevents,
                        };
                        render_help(v_sections[1], f, history_recording);
                    } else if show_section_mgr {
                        let v_sections = Layout::default()
                            .direction(Direction::Vertical)
                            .margin(0)
                            .constraints([Constraint::Length(1), Constraint::Length(40)].as_ref())
                            .split(f.size());
                        render_top_title_bar(app, v_sections[0], f, zf, offset);
                        render_section_mgr(section_manager_options, v_sections[1], f);
                    } else {
                        // create layouts
                        // primary vertical
                        let v_sections = Layout::default()
                            .direction(Direction::Vertical)
                            .margin(0)
                            .constraints(constraints.as_ref())
                            .split(f.size());

                        render_top_title_bar(app, v_sections[0], f, zf, offset);
                        let view = View {
                            zoom_factor: *zf,
                            update_number: *un,
                            width: 0,
                            offset: *offset,
                        };
                        for section_index in 0..geometry.len() {
                            let v_section = v_sections[section_index + 1];
                            let current_section = geometry[section_index].0;
                            let border_style = if current_section == selected {
                                Style::default().fg(Color::Red)
                            } else {
                                Style::default()
                            };
                            match current_section {
                                Section::Cpu => render_cpu(app, v_section, f, view, border_style),
                                Section::Network => {
                                    render_net(app, v_section, f, view, border_style)
                                }
                                Section::Disk => render_disk(
                                    app,
                                    v_section,
                                    f,
                                    view,
                                    border_style,
                                    file_system_index,
                                    file_system_display,
                                ),
                                Section::Graphics => render_graphics(
                                    app,
                                    v_section,
                                    f,
                                    view,
                                    gfx_device_index,
                                    border_style,
                                ),
                                Section::Process => {
                                    if let Some(p) = app.selected_process.as_ref() {
                                        render_process(
                                            app,
                                            v_section,
                                            f,
                                            border_style,
                                            process_message,
                                            p,
                                        );
                                    } else {
                                        highlighted_process = render_process_table(
                                            app,
                                            &process_table,
                                            v_section,
                                            *pst,
                                            f,
                                            border_style,
                                            show_paths,
                                            show_find,
                                            filter,
                                            highlighted_row,
                                        );
                                        if v_section.height > 4 {
                                            // account for table border & margins.
                                            process_table_height = v_section.height - 5;
                                        }
                                    }
                                }
                            }
                        }
                    }
                })
                .expect("Could not draw frame.");

            let event = self.events.next().expect("No new event.");
            let action = match event {
                Event::Input(input) => {
                    let process_table = process_table.into_owned();
                    self.process_key_event(
                        input,
                        &process_table,
                        process_table_height,
                        highlighted_process,
                    )
                    .await
                }
                Event::Resize(_, height) => {
                    self.constraints = get_constraints(&self.section_geometry, height);
                    Action::Continue
                }
                Event::Tick => {
                    self.process_tick().await;
                    Action::Continue
                }
                Event::Save => {
                    debug!("Event Save");
                    self.app.save_state().await;
                    Action::Continue
                }
                Event::Terminate => {
                    debug!("Event Terminate");
                    Action::Quit
                }
            };
            match action {
                Action::Quit => break,
                Action::Continue => {}
            }
        }
    }

    async fn process_tick(&mut self) {
        debug!("Event Tick");

        if self.app.selected_process.is_none() {
            if let Some(start) = self.selection_grace_start {
                if start.elapsed() > PROCESS_SELECTION_GRACE {
                    self.selection_grace_start = None;
                }
            }
        }

        let keep_order =
            self.app.selected_process.is_some() || self.selection_grace_start.is_some();

        self.app.update(keep_order).await;
        self.update_number += 1;
        if self.update_number == self.zoom_factor {
            self.update_number = 0;
        }
    }

    async fn process_key_event(
        &mut self,
        input: KeyEvent,
        process_table: &[i32],
        process_table_height: u16,
        highlighted_process: Option<Box<ZProcess>>,
    ) -> Action {
        debug!("Event Key: {:?}", input);
        match input.code {
            Key::Up => self.view_up(process_table, 1),
            Key::PageUp => self.view_up(process_table, process_table_height.into()),
            Key::Down => self.view_down(process_table, process_table_height.into(), 1),
            Key::PageDown => self.view_down(
                process_table,
                process_table_height.into(),
                process_table_height.into(),
            ),
            Key::Home => self.view_up(process_table, process_table.len()),
            Key::End => self.view_down(
                process_table,
                process_table_height.into(),
                process_table.len(),
            ),
            Key::Left => self.histogram_left(),
            Key::Right => self.histogram_right(),
            Key::Enter => self.select(highlighted_process),
            Key::Char('c') => {
                if input.modifiers.contains(KeyModifiers::CONTROL) {
                    return Action::Quit;
                } else if self.show_find {
                    self.process_find_input(input);
                }
            }
            _other => {
                if self.show_find {
                    self.process_find_input(input);
                } else {
                    return self.process_toplevel_input(input).await;
                }
            }
        };
        Action::Continue
    }

    fn select(&mut self, highlighted_process: Option<Box<ZProcess>>) {
        let selected = self.selected_section();
        if selected == Section::Process {
            self.app.select_process(highlighted_process);
            self.process_message = None;
            self.show_find = false;
            self.process_table_row_start = 0;
        }
    }

    fn view_up(&mut self, process_table: &[i32], delta: usize) {
        let selected = self.selected_section();
        if self.show_section_mgr {
            match self.section_manager_options.state.selected() {
                Some(i) => {
                    let mut idx = 0;
                    if (i as i32 - delta as i32) > 0 {
                        idx = i - delta;
                    }
                    self.section_manager_options.state.select(Some(idx));
                }
                None => self.section_manager_options.state.select(Some(0)),
            }
        } else if selected == Section::Graphics {
            if self.gfx_device_index > 0 {
                self.gfx_device_index -= 1;
            }
        } else if selected == Section::Disk {
            if self.file_system_index > 0 {
                self.file_system_index -= 1;
            }
        } else if selected == Section::Process {
            if self.app.selected_process.is_some() || process_table.is_empty() {
                return;
            }

            self.selection_grace_start = Some(Instant::now());
            if self.highlighted_row != 0 {
                self.highlighted_row = self.highlighted_row.saturating_sub(delta);
            }
            if self.process_table_row_start > 0
                && self.highlighted_row < self.process_table_row_start
            {
                self.process_table_row_start = self.process_table_row_start.saturating_sub(delta);
            }
        }
    }

    fn view_down(&mut self, process_table: &[i32], process_table_height: usize, delta: usize) {
        use std::cmp::min;
        let selected = self.selected_section();
        if self.show_section_mgr {
            match self.section_manager_options.state.selected() {
                Some(i) => {
                    let mut idx = self.section_manager_options.items.len() - 1;
                    if i + delta < idx {
                        idx = i + delta;
                    }
                    self.section_manager_options.state.select(Some(idx));
                }
                None => self.section_manager_options.state.select(Some(0)),
            }
        } else if selected == Section::Graphics {
            if self.gfx_device_index < self.app.gfx_devices.len() - 1 {
                self.gfx_device_index += 1;
            }
        } else if selected == Section::Disk {
            if self.file_system_index < self.app.disks.len() - 1 {
                self.file_system_index += 1;
            }
        } else if selected == Section::Process {
            if self.app.selected_process.is_some() || process_table.is_empty() {
                return;
            }

            self.selection_grace_start = Some(Instant::now());
            if self.highlighted_row < process_table.len() - 1 {
                self.highlighted_row = min(self.highlighted_row + delta, process_table.len() - 1);
            }
            if self.process_table_row_start < process_table.len()
                && self.highlighted_row > (self.process_table_row_start + process_table_height)
            {
                self.process_table_row_start = min(
                    self.process_table_row_start + delta,
                    process_table.len() - process_table_height - 1,
                );
            }
        }
    }

    fn histogram_left(&mut self) {
        if let Some(w) = self.app.histogram_map.histograms_width() {
            self.hist_start_offset += 1;
            if self.hist_start_offset > w + 1 {
                self.hist_start_offset = w - 1;
            }
        }
        self.hist_start_offset += 1;
    }

    fn histogram_right(&mut self) {
        if self.hist_start_offset > 0 {
            self.hist_start_offset -= 1;
        }
    }

    fn process_find_input(&mut self, input: KeyEvent) {
        match input.code {
            Key::Esc => {
                self.show_find = false;
                self.filter = String::from("");
            }
            Key::Char(c) if c != '\n' => {
                self.selection_grace_start = Some(Instant::now());
                self.filter.push(c)
            }
            Key::Delete => match self.filter.pop() {
                Some(_c) => {}
                None => self.show_find = false,
            },
            Key::Backspace => match self.filter.pop() {
                Some(_c) => {}
                None => self.show_find = false,
            },
            _ => {}
        }
    }

    fn recompute_constraints(&mut self) {
        self.selected_section_index = 0;
        for idx in 0..self.section_geometry.len() {
            self.section_geometry[idx].1 = 100.0 / self.section_geometry.len() as f64;
        }
        let new_geometry = self.section_geometry.clone();
        let selected = self.section_manager_options.state.selected();
        self.section_manager_options = SectionMGRList::with_geometry(new_geometry);
        self.section_manager_options.state.select(selected);
        self.constraints = get_constraints(self.section_geometry.as_slice(), terminal_size().1);
    }

    fn toggle_section(&mut self) {
        if self.show_section_mgr {
            if let Some(s) = self.section_manager_options.selected() {
                if self.section_geometry.len() > 1
                    && self.section_geometry.iter().any(|(gs, _)| *gs == s)
                {
                    self.section_geometry.retain(|(section, _)| *section != s);
                    self.recompute_constraints();
                } else if !self.section_geometry.iter().any(|(gs, _)| *gs == s) {
                    let idx = 0;
                    self.section_geometry.insert(idx, (s, 1.0));
                    self.section_geometry
                        .sort_by(|(a_section, _), (b_section, _)| {
                            a_section
                                .partial_cmp(b_section)
                                .expect("Can't compare sections. Shouldn't happen.")
                        });
                    self.recompute_constraints();
                }
            }
        }
    }

    fn toggle_section_mgr(&mut self) {
        self.show_section_mgr = !self.show_section_mgr;
    }

    async fn process_toplevel_input(&mut self, input: KeyEvent) -> Action {
        match input.code {
            Key::Char('q') => {
                return Action::Quit;
            }
            Key::Char('.') | Key::Char('>') => {
                if self.app.psortby == ProcessTableSortBy::Cmd {
                    self.app.psortby = ProcessTableSortBy::Pid;
                } else {
                    self.app.psortby = FromPrimitive::from_u32(self.app.psortby as u32 + 1)
                        .expect("invalid value to set psortby");
                }
                self.app.sort_process_table();
            }
            Key::Char(',') | Key::Char('<') => {
                if self.app.psortby == ProcessTableSortBy::Pid {
                    self.app.psortby = ProcessTableSortBy::Cmd;
                } else {
                    self.app.psortby = FromPrimitive::from_u32(self.app.psortby as u32 - 1)
                        .expect("invalid value to set psortby");
                }
                self.app.sort_process_table();
            }
            Key::Char(';') => {
                match self.app.psortorder {
                    ProcessTableSortOrder::Ascending => {
                        self.app.psortorder = ProcessTableSortOrder::Descending
                    }
                    ProcessTableSortOrder::Descending => {
                        self.app.psortorder = ProcessTableSortOrder::Ascending
                    }
                }
                self.app.sort_process_table();
            }
            Key::Char('+') | Key::Char('=') => {
                if self.zoom_factor > 1 {
                    self.zoom_factor -= 1;
                }
                self.update_number = 0;
            }
            Key::Char('-') => {
                if self.zoom_factor < 100 {
                    self.zoom_factor += 1;
                }
                self.update_number = 0;
            }
            Key::Esc | Key::Char('b') => {
                self.app.selected_process = None;
                self.process_message = None;
            }
            Key::Char('s') => {
                self.process_message = match &self.app.selected_process {
                    Some(p) => Some(p.suspend().await),
                    None => None,
                };
            }
            Key::Char('r') => {
                self.process_message = match &self.app.selected_process {
                    Some(p) => Some(p.resume().await),
                    None => None,
                };
            }
            Key::Char('k') => {
                self.process_message = match &self.app.selected_process {
                    Some(p) => Some(p.kill().await),
                    None => None,
                };
            }
            Key::Char('t') => {
                self.process_message = match &self.app.selected_process {
                    Some(p) => Some(p.terminate().await),
                    None => None,
                };
            }
            Key::Char('n') => {
                self.process_message = self.app.selected_process.as_mut().map(|p| p.nice());
            }
            Key::Char('p') if self.app.selected_process.is_some() => {
                self.process_message = self
                    .app
                    .selected_process
                    .as_mut()
                    .map(|p| p.set_priority(0));
            }
            k @ Key::Tab | k @ Key::BackTab => {
                // hopefully cross platform enough regarding https://github.com/crossterm-rs/crossterm/issues/442
                self.selected_section_index =
                    if k == Key::BackTab || input.modifiers.contains(KeyModifiers::SHIFT) {
                        match self.selected_section_index {
                            0 => self.section_geometry.len() - 1,
                            x => x - 1,
                        }
                    } else {
                        (self.selected_section_index + 1) % self.section_geometry.len()
                    };
            }
            Key::Char(' ') => {
                self.toggle_section();
            }
            Key::F(1) => {
                self.toggle_section_mgr();
            }
            Key::Char('m') => {
                self.update_section_height(-2).await;
            }
            Key::Char('e') => {
                self.update_section_height(2).await;
            }
            Key::Char('`') => {
                self.zoom_factor = 1;
                self.hist_start_offset = 0;
            }
            Key::Char('h') => {
                self.show_help = !self.show_help;
            }
            Key::Char('p') => {
                self.show_paths = !self.show_paths;
            }
            Key::Char('/') => {
                self.show_find = true;
                self.highlighted_row = 0;
                self.process_table_row_start = 0;
            }
            Key::Char('a') => {
                if self.file_system_display == FileSystemDisplay::Activity {
                    self.file_system_display = FileSystemDisplay::Usage;
                } else {
                    self.file_system_display = FileSystemDisplay::Activity;
                }
            }
            _ => {}
        }

        Action::Continue
    }
}

#[must_use]
enum Action {
    Continue,
    Quit,
}

enum HistoryRecording {
    On,
    UserDisabled,
    OtherInstancePrevents,
}
