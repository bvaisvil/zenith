/**
 * Copyright 2019-2020, Benjamin Vaisvil and the zenith contributors
 */
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
use std::io;
use std::io::Stdout;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant, UNIX_EPOCH};
use tui::{backend::CrosstermBackend, Terminal};

use std::ops::Mul;
use tui::backend::Backend;
use tui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use tui::style::{Color, Modifier, Style};
use tui::text::{Span, Spans};
use tui::widgets::{
    BarChart, Block, Borders, List, ListItem, Paragraph, Row, Sparkline, Table, Wrap,
};
use tui::Frame;

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

macro_rules! set_section_height {
    ($x:expr, $val:expr) => {
        if $x + $val > 0 {
            $x += $val;
        }
    };
}

#[derive(FromPrimitive, PartialEq, Copy, Clone)]
enum Section {
    CPU = 0,
    Network = 1,
    Disk = 2,
    Graphics = 3,
    Process = 4,
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
        let t: f32 = app.sensors.iter().map(|s| s.current_temp).sum();
        format!("TEMP [{: >3.0}°C]", t)
    } else {
        String::from("")
    };
    format!(
        "CPU [{: >3}%] {:} MEAN [{: >3.2}%] TOP [{} - {} - {}]",
        app.cpu_utilization, temp, mean, top_pid, top_process_name, top_process_amt
    )
}

fn render_process_table(
    app: &CPUTimeApp,
    process_table: &[i32],
    width: u16,
    area: Rect,
    process_table_start: usize,
    f: &mut Frame<'_, ZBackend>,
    selected_section: &Section,
    show_paths: bool,
    show_find: bool,
    filter: &str,
    highlighted_row: usize,
) -> Option<ZProcess> {
    let style = match selected_section {
        Section::Process => Style::default().fg(Color::Red),
        _ => Style::default(),
    };
    let display_height = if area.height > 4 {
        area.height as usize - 4 // 4 for the margins and table header
    } else {
        0
    };
    if display_height == 0 {
        return None;
    }

    let procs: Vec<&ZProcess> = process_table
        .iter()
        .map(|pid| {
            app.process_map
                .get(pid)
                .expect("expected pid to be present")
        })
        .collect();
    let highlighted_process = if !procs.is_empty() {
        Some(procs[highlighted_row].clone())
    } else {
        None
    };
    if area.height < 5 {
        return highlighted_process; // not enough space to draw anything
    }
    let rows: Vec<(Vec<String>, Option<Style>)> = procs
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

            let style = if i == highlighted_row {
                Some(
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                None
            };

            (row, style)
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
    let cmd_width = width.saturating_sub(used_width).saturating_sub(3);
    let cmd_header = format!("{:<width$}", "CMD", width = cmd_width as usize);
    widths.push(Constraint::Min(cmd_width));
    header.push(cmd_header);

    header[app.psortby as usize].pop();
    let sort_ind = match app.psortorder {
        ProcessTableSortOrder::Ascending => '↑',
        ProcessTableSortOrder::Descending => '↓',
    };
    header[app.psortby as usize].insert(0, sort_ind); //sort column indicator
    let rows_view = rows.iter().map(|(row, style)| {
        if let Some(style) = style {
            Row::StyledData(row.iter(), *style)
        } else {
            Row::Data(row.iter())
        }
    });

    let title = if show_find {
        format!("[ESC] Clear, Find: {:}", filter)
    } else if !filter.is_empty() {
        format!("Filtered Results: {:}, [f] to change/clear", filter)
    } else {
        format!(
            "Tasks [{:}] Threads [{:}]  Navigate [↑/↓] Sort Col [,/.] Asc/Dec [/] Filter [f]",
            app.processes.len(),
            app.threads_total
        )
    };

    Table::new(header.into_iter(), rows_view)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(style)
                .title(Span::styled(title, style)),
        )
        .widths(widths.as_slice())
        .column_spacing(0)
        .header_style(Style::default().bg(Color::DarkGray))
        .render(f, area);
    highlighted_process
}

fn render_cpu_histogram(
    app: &CPUTimeApp,
    area: Rect,
    f: &mut Frame<'_, ZBackend>,
    zf: &u32,
    update_number: &u32,
    offset: &usize,
) {
    let h = match app.histogram_map.get_zoomed(
        "cpu_usage_histogram",
        *zf,
        *update_number,
        area.width as usize,
        *offset,
    ) {
        Some(h) => h.data,
        None => return,
    };
    let title = cpu_title(&app, &h);
    Sparkline::default()
        .block(Block::default().title(title.as_str()))
        .data(&h)
        .style(Style::default().fg(Color::Blue))
        .max(100)
        .render(f, area);
}

fn render_memory_histogram(
    app: &CPUTimeApp,
    area: Rect,
    f: &mut Frame<'_, ZBackend>,
    zf: &u32,
    update_number: &u32,
    offset: &usize,
) {
    let h = match app.histogram_map.get_zoomed(
        "mem_utilization",
        *zf,
        *update_number,
        area.width as usize,
        *offset,
    ) {
        Some(h) => h.data,
        None => return,
    };
    let title2 = mem_title(&app);
    Sparkline::default()
        .block(Block::default().title(title2.as_str()))
        .data(&h)
        .style(Style::default().fg(Color::Cyan))
        .max(100)
        .render(f, area);
}

fn render_cpu_bars(
    app: &CPUTimeApp,
    area: Rect,
    width: u16,
    f: &mut Frame<'_, ZBackend>,
    style: &Style,
) {
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

    assert_eq!(area.width, width);

    let layout = Layout::default().margin(1).direction(Direction::Vertical);

    if full_width > 2 * width {
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

    if full_width <= width {
        // fits in one row

        let cpu_bar_layout = layout
            .constraints(vec![Constraint::Percentage(100)])
            .split(area);

        let bar_width = clamp_up((width - (core_count - 1)) / core_count, max_bar_width);

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

        let bar_width = clamp_up((width * 2 - (core_count - 1)) / core_count, max_bar_width);

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
    zf: &u32,
    update_number: &u32,
    offset: &usize,
    selected_section: &Section,
) {
    let style = match selected_section {
        Section::Network => Style::default().fg(Color::Red),
        _ => Style::default(),
    };
    Block::default()
        .title("Network")
        .borders(Borders::ALL)
        .border_style(style)
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

    let net_up = float_to_byte_string!(
        app.net_out as f64 / app.histogram_map.tick.as_secs_f64(),
        ByteUnit::B
    );
    let h_out = match app.histogram_map.get_zoomed(
        "net_out",
        *zf,
        *update_number,
        net[0].width as usize,
        *offset,
    ) {
        Some(h) => h.data,
        None => return,
    };

    let up_max: u64 = match h_out.iter().max() {
        Some(x) => *x,
        None => 1,
    };
    let up_max_bytes = float_to_byte_string!(up_max as f64, ByteUnit::B);

    Sparkline::default()
        .block(
            Block::default()
                .title(format!("↑ [{:^10}/s] Max [{:^10}/s]", net_up, up_max_bytes).as_str()),
        )
        .data(&h_out)
        .style(Style::default().fg(Color::LightYellow))
        .max(up_max)
        .render(f, net[0]);

    let net_down = float_to_byte_string!(
        app.net_in as f64 / app.histogram_map.tick.as_secs_f64(),
        ByteUnit::B
    );
    let h_in = match app.histogram_map.get_zoomed(
        "net_in",
        *zf,
        *update_number,
        net[1].width as usize,
        *offset,
    ) {
        Some(h) => h.data,
        None => return,
    };

    let down_max: u64 = match h_in.iter().max() {
        Some(x) => *x,
        None => 1,
    };
    let down_max_bytes = float_to_byte_string!(down_max as f64, ByteUnit::B);
    Sparkline::default()
        .block(
            Block::default()
                .title(format!("↓ [{:^10}/s] Max [{:^10}/s]", net_down, down_max_bytes).as_str()),
        )
        .data(&h_in)
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
                .title(Span::styled("Network", style))
                .borders(Borders::ALL)
                .border_style(style),
        )
        .render(f, network_layout[0]);
}

fn render_process(
    app: &CPUTimeApp,
    layout: Rect,
    f: &mut Frame<'_, ZBackend>,
    selected_section: &Section,
    process_message: &Option<String>,
) {
    let style = match selected_section {
        Section::Process => Style::default().fg(Color::Red),
        _ => Style::default(),
    };
    let p = match &app.selected_process {
        Some(p) => p,
        None => return,
    };
    Block::default()
        .title(Span::styled(format!("Process: {0}", p.name), style))
        .borders(Borders::ALL)
        .border_style(style)
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

fn render_disk(
    app: &CPUTimeApp,
    layout: Rect,
    f: &mut Frame<'_, ZBackend>,
    zf: &u32,
    update_number: &u32,
    offset: &usize,
    selected_section: &Section,
) {
    let style = match selected_section {
        Section::Disk => Style::default().fg(Color::Red),
        _ => Style::default(),
    };
    Block::default()
        .title("Disk")
        .borders(Borders::ALL)
        .border_style(style)
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

    let read_up = float_to_byte_string!(app.disk_read as f64, ByteUnit::B);
    let h_read = match app.histogram_map.get_zoomed(
        "disk_read",
        *zf,
        *update_number,
        area[0].width as usize,
        *offset,
    ) {
        Some(h) => h.data,
        None => return,
    };

    let read_max: u64 = match h_read.iter().max() {
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
        .data(&h_read)
        .style(Style::default().fg(Color::LightYellow))
        .max(read_max)
        .render(f, area[0]);

    let write_down = float_to_byte_string!(app.disk_write as f64, ByteUnit::B);
    let h_write = match app.histogram_map.get_zoomed(
        "disk_write",
        *zf,
        *update_number,
        area[1].width as usize,
        *offset,
    ) {
        Some(h) => h.data,
        None => return,
    };

    let write_max: u64 = match h_write.iter().max() {
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
                    "W [{:^10}/s] Max [{:^10}/s] {:}",
                    write_down, write_max_bytes, top_writer
                )
                .as_str(),
            ),
        )
        .data(&h_write)
        .style(Style::default().fg(Color::LightMagenta))
        .max(write_max)
        .render(f, area[1]);
    let disks: Vec<_> = app
        .disks
        .iter()
        .map(|d| {
            let style = if d.get_perc_free_space() < 10.0 {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            };
            Span::styled(
                Cow::Owned(format!(
                    "{:3.0}%: {}",
                    d.get_perc_free_space(),
                    d.mount_point.display()
                )),
                style,
            )
        })
        .map(ListItem::new)
        .collect();
    List::new(disks)
        .block(
            Block::default()
                .title(Span::styled("Disks / File Systems", style))
                .borders(Borders::ALL)
                .border_style(style),
        )
        .render(f, disk_layout[0]);
}

fn render_graphics(
    app: &CPUTimeApp,
    layout: Rect,
    f: &mut Frame<'_, ZBackend>,
    zf: &u32,
    update_number: &u32,
    gfx_device_index: &usize,
    offset: &usize,
    selected_section: &Section,
) {
    let style = match selected_section {
        Section::Graphics => Style::default().fg(Color::Red),
        _ => Style::default(),
    };
    Block::default()
        .title("Graphics")
        .borders(Borders::ALL)
        .border_style(style)
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
    let gd = &app.gfx_devices[*gfx_device_index];
    let h_gpu = match app.histogram_map.get_zoomed(
        format!("{}_gpu", gd.uuid).as_str(),
        *zf,
        *update_number,
        area[0].width as usize,
        *offset,
    ) {
        Some(h) => h.data,
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
        .data(&h_gpu)
        .style(Style::default().fg(Color::LightYellow))
        .max(100)
        .render(f, area[0]);

    let h_mem = match app.histogram_map.get_zoomed(
        format!("{}_mem", gd.uuid).as_str(),
        *zf,
        *update_number,
        area[1].width as usize,
        *offset,
    ) {
        Some(h) => h.data,
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
        .data(&h_mem)
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
                .title(Span::styled("Graphics Devices", style))
                .borders(Borders::ALL)
                .border_style(style),
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
    let line = vec![
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
        Span::styled(
            format!("{: >width$}", "", width = area.width as usize),
            default_style,
        ),
    ];
    Paragraph::new(Spans::from(line)).render(f, area);
}

fn render_cpu(
    app: &CPUTimeApp,
    area: Rect,
    f: &mut Frame<'_, ZBackend>,
    zf: &u32,
    update_number: &u32,
    offset: &usize,
    selected_section: &Section,
) {
    let style = match selected_section {
        Section::CPU => Style::default().fg(Color::Red),
        _ => Style::default(),
    };
    Block::default()
        .title("")
        .borders(Borders::ALL)
        .border_style(style)
        .render(f, area);
    let cpu_layout = Layout::default()
        .margin(0)
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(LEFT_PANE_WIDTH), Constraint::Min(10)].as_ref())
        .split(area);

    let cpu_mem = Layout::default()
        .margin(1)
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(cpu_layout[1]);
    render_cpu_histogram(&app, cpu_mem[0], f, zf, update_number, offset);
    render_memory_histogram(&app, cpu_mem[1], f, zf, update_number, offset);
    render_cpu_bars(&app, cpu_layout[0], LEFT_PANE_WIDTH, f, &style);
}

fn filter_process_table(app: &CPUTimeApp, filter: &str) -> Vec<i32> {
    let filter_lc = filter.to_lowercase();
    let results: Vec<i32> = app
        .processes
        .iter()
        .filter(|pid| {
            let p = app
                .process_map
                .get(pid)
                .expect("Pid present in processes but not in map.");
            filter.is_empty()
                || p.name.to_lowercase().contains(&filter_lc)
                || p.exe.to_lowercase().contains(&filter_lc)
                || p.command.join(" ").to_lowercase().contains(&filter_lc)
                || format!("{:}", p.pid).contains(&filter_lc)
        })
        .copied()
        .collect();
    results
}

fn render_help(area: Rect, f: &mut Frame<'_, ZBackend>) {
    let help_layout = Layout::default()
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
    let t = vec![Span::styled(
        format!("zenith v{:}", env!("CARGO_PKG_VERSION")),
        header_style,
    )];
    Paragraph::new(Spans::from(t))
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Center)
        .render(f, help_layout[0]);
    let main_style = Style::default();
    let key_style = main_style.fg(Color::Cyan);

    let t = vec![
        Spans::from(vec![Span::styled("Primary Interface", header_style)]),
        Spans::from(vec![
            Span::styled("h    ", key_style),
            Span::styled("    Toggle this help screen\n", main_style),
        ]),
        Spans::from(vec![
            Span::styled("q    ", key_style),
            Span::styled("    Quit and exit zenith\n", main_style),
        ]),
        Spans::from(vec![
            Span::styled("<TAB>", key_style),
            Span::styled("    Changes highlighted section\n", main_style),
        ]),
        Spans::from(vec![
            Span::styled("e    ", key_style),
            Span::styled("    Expands highlighted section\n", main_style),
        ]),
        Spans::from(vec![
            Span::styled("m    ", key_style),
            Span::styled("    Shrinks highlighted section\n", main_style),
        ]),
        Spans::from(vec![
            Span::styled("-    ", key_style),
            Span::styled("    Zoom chart out\n", main_style),
        ]),
        Spans::from(vec![
            Span::styled("+    ", key_style),
            Span::styled("    Zoom chart in\n", main_style),
        ]),
        Spans::from(vec![
            Span::styled("←    ", key_style),
            Span::styled("    Move back in time\n", main_style),
        ]),
        Spans::from(vec![
            Span::styled("→    ", key_style),
            Span::styled("    Move forward In time\n", main_style),
        ]),
        Spans::from(vec![
            Span::styled("`    ", key_style),
            Span::styled("    Reset charts to current\n", main_style),
        ]),
        Spans::from(vec![Span::styled("", header_style)]),
        Spans::from(vec![Span::styled("Process Table\n", header_style)]),
        Spans::from(vec![
            Span::styled("<RET> ", key_style),
            Span::styled("    Focus current process\n", main_style),
        ]),
        Spans::from(vec![
            Span::styled("↓     ", key_style),
            Span::styled("    Move one line down\n", main_style),
        ]),
        Spans::from(vec![
            Span::styled("↑     ", key_style),
            Span::styled("    Move one line up\n", main_style),
        ]),
        Spans::from(vec![
            Span::styled("PgDown", key_style),
            Span::styled("    Move view one screen down\n", main_style),
        ]),
        Spans::from(vec![
            Span::styled("PgUp  ", key_style),
            Span::styled("    Move view one screen up\n", main_style),
        ]),
        Spans::from(vec![
            Span::styled("/     ", key_style),
            Span::styled("    Change sort between ascending/descending\n", main_style),
        ]),
        Spans::from(vec![
            Span::styled(",     ", key_style),
            Span::styled("    Cycle columns left\n", main_style),
        ]),
        Spans::from(vec![
            Span::styled(".     ", key_style),
            Span::styled("    Cycle columns right\n", main_style),
        ]),
        Spans::from(vec![
            Span::styled("p     ", key_style),
            Span::styled("    Toggle paths on/off\n", main_style),
        ]),
        Spans::from(vec![
            Span::styled("f     ", key_style),
            Span::styled("    Toggle filter mode\n", main_style),
        ]),
        Spans::from(vec![
            Span::styled("<ESC> ", key_style),
            Span::styled("    Leave filter mode\n", main_style),
        ]),
    ];
    let b = Block::default().borders(Borders::ALL);
    Paragraph::new(t)
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Left)
        .block(b)
        .render(f, help_layout[1]);
}

pub struct TerminalRenderer {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    app: CPUTimeApp,
    events: Events,
    process_table_row_start: usize,
    gfx_device_index: usize,
    cpu_height: i16,
    net_height: i16,
    disk_height: i16,
    process_height: i16,
    graphics_height: i16,
    _sensor_height: i16,
    zoom_factor: u32,
    update_number: u32,
    hist_start_offset: usize,
    selected_section: Section,
    constraints: Vec<Constraint>,
    process_message: Option<String>,
    show_help: bool,
    show_paths: bool,
    show_find: bool,
    filter: String,
    highlighted_row: usize,
    selection_grace_start: Option<Instant>,
}

impl<'a> TerminalRenderer {
    pub fn new(
        tick_rate: u64,
        cpu_height: i16,
        net_height: i16,
        disk_height: i16,
        process_height: i16,
        sensor_height: i16,
        graphics_height: i16,
        db_path: Option<PathBuf>,
    ) -> TerminalRenderer {
        debug!("Setup Constraints");
        let mut constraints = vec![
            Constraint::Length(1),
            Constraint::Length(cpu_height as u16),
            Constraint::Length(net_height as u16),
            Constraint::Length(disk_height as u16),
            Constraint::Length(graphics_height as u16),
        ];
        if process_height > 0 {
            constraints.push(Constraint::Min(process_height as u16));
        }

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

        TerminalRenderer {
            terminal,
            app,
            events,
            process_table_row_start: 0,
            gfx_device_index: 0,
            cpu_height,
            net_height,
            disk_height,
            process_height,
            graphics_height,
            _sensor_height: sensor_height, // unused at the moment
            zoom_factor: 1,
            update_number: 0,
            selected_section: Section::Process,
            constraints,
            process_message: None,
            hist_start_offset: 0,
            show_help: false,
            show_paths: false,
            show_find: false,
            filter: String::from(""),
            highlighted_row: 0,
            selection_grace_start: None,
        }
    }

    async fn set_constraints(&mut self) {
        let mut constraints = vec![
            Constraint::Length(1),
            Constraint::Length(self.cpu_height as u16),
            Constraint::Length(self.net_height as u16),
            Constraint::Length(self.disk_height as u16),
            Constraint::Length(self.graphics_height as u16),
        ];
        if self.process_height > 0 {
            constraints.push(Constraint::Min(self.process_height as u16));
        }
        self.constraints = constraints;
    }

    async fn set_section_height(&mut self, val: i16) {
        match self.selected_section {
            Section::CPU => set_section_height!(self.cpu_height, val),
            Section::Disk => set_section_height!(self.disk_height, val),
            Section::Network => set_section_height!(self.net_height, val),
            Section::Graphics => set_section_height!(self.graphics_height, val),
            Section::Process => set_section_height!(self.process_height, val),
        }
        self.set_constraints().await;
    }

    fn current_section_height(&mut self) -> i16 {
        match self.selected_section {
            Section::CPU => self.cpu_height,
            Section::Disk => self.disk_height,
            Section::Network => self.net_height,
            Section::Graphics => self.graphics_height,
            Section::Process => self.process_height,
        }
    }

    pub async fn start(&mut self) {
        debug!("Starting Main Loop.");
        loop {
            let app = &self.app;
            let pst = &self.process_table_row_start;
            let process_height = &self.process_height;
            let mut width: u16 = 0;
            let mut process_table_height: u16 = 0;
            let zf = &self.zoom_factor;
            let constraints = &self.constraints;
            let selected = &self.selected_section;
            let process_message = &self.process_message;
            let offset = &self.hist_start_offset;
            let un = &self.update_number;
            let show_help = self.show_help;
            let show_paths = self.show_paths;
            let filter = &self.filter;
            let show_find = self.show_find;
            let mut highlighted_process: Option<ZProcess> = None;
            let process_table = filter_process_table(app, &self.filter);
            let gfx_device_index = &self.gfx_device_index;

            if !process_table.is_empty() && self.highlighted_row >= process_table.len() {
                self.highlighted_row = process_table.len() - 1;
            }
            let highlighted_row = self.highlighted_row;

            self.terminal
                .draw(|mut f| {
                    width = f.size().width;
                    if show_help {
                        let v_sections = Layout::default()
                            .direction(Direction::Vertical)
                            .margin(0)
                            .constraints([Constraint::Length(1), Constraint::Length(40)].as_ref())
                            .split(f.size());

                        render_top_title_bar(app, v_sections[0], &mut f, zf, offset);
                        render_help(v_sections[1], &mut f);
                    } else {
                        // create layouts
                        // primary vertical
                        let v_sections = Layout::default()
                            .direction(Direction::Vertical)
                            .margin(0)
                            .constraints(constraints.as_ref())
                            .split(f.size());

                        render_top_title_bar(app, v_sections[0], &mut f, zf, offset);
                        render_cpu(app, v_sections[1], &mut f, zf, un, offset, selected);
                        render_net(&app, v_sections[2], &mut f, zf, un, offset, selected);
                        render_disk(&app, v_sections[3], &mut f, zf, un, offset, selected);
                        render_graphics(
                            &app,
                            v_sections[4],
                            &mut f,
                            zf,
                            un,
                            gfx_device_index,
                            offset,
                            selected,
                        );

                        if *process_height > 0 {
                            if let Some(area) = v_sections.last() {
                                if app.selected_process.is_none() {
                                    highlighted_process = render_process_table(
                                        &app,
                                        &process_table,
                                        width,
                                        *area,
                                        *pst,
                                        &mut f,
                                        selected,
                                        show_paths,
                                        show_find,
                                        filter,
                                        highlighted_row,
                                    );
                                    if area.height > 4 {
                                        // account for table border & margins.
                                        process_table_height = area.height - 5;
                                    }
                                } else if app.selected_process.is_some() {
                                    render_process(&app, *area, &mut f, selected, process_message);
                                }
                            }
                        }
                    }

                    //render_sensors(&app, sensor_layout, &mut f, zf);
                })
                .expect("Could not draw frame.");

            match self
                .process_next_event(
                    &process_table,
                    process_table_height,
                    highlighted_process,
                    width,
                )
                .await
            {
                Action::Quit => break,
                Action::Continue => {}
            }
        }
    }

    async fn process_next_event(
        &mut self,
        process_table: &[i32],
        process_table_height: u16,
        highlighted_process: Option<ZProcess>,
        width: u16,
    ) -> Action {
        let input = match self.events.next().expect("No new event.") {
            Event::Input(input) => input,
            Event::Tick => {
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

                self.app.update(width, keep_order).await;
                self.update_number += 1;
                if self.update_number == self.zoom_factor {
                    self.update_number = 0;
                }
                return Action::Continue;
            }
            Event::Save => {
                debug!("Event Save");
                self.app.save_state().await;
                return Action::Continue;
            }
            Event::Terminate => {
                debug!("Event Terminate");
                return Action::Quit;
            }
        };

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
            Key::Left => self.histogram_left(),
            Key::Right => self.histogram_right(),
            Key::Enter => {
                self.app.select_process(highlighted_process);
                self.process_message = None;
                self.show_find = false;
                self.process_table_row_start = 0;
            }
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

    fn view_up(&mut self, process_table: &[i32], delta: usize) {
        if self.selected_section == Section::Graphics {
            if self.gfx_device_index > 0 {
                self.gfx_device_index -= 1;
            }
        } else if self.selected_section == Section::Process {
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
        if self.selected_section == Section::Graphics {
            if self.gfx_device_index < self.app.gfx_devices.len() - 1 {
                self.gfx_device_index += 1;
            }
        } else if self.selected_section == Section::Process {
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

    fn advance_to_next_section(&mut self) {
        if self.cpu_height > 0
            || self.net_height > 0
            || self.disk_height > 0
            || self.graphics_height > 0
            || self.process_height > 0
        {
            let mut i = self.selected_section as u32 + 1;
            if i > 4 {
                i = 0;
            }
            self.selected_section = FromPrimitive::from_u32(i).unwrap_or(Section::CPU);
            if self.current_section_height() == 0 {
                self.advance_to_next_section();
            }
        }
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
            Key::Char('/') => {
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
                self.process_message = match &mut self.app.selected_process {
                    Some(p) => Some(p.nice()),
                    None => None,
                };
            }
            Key::Char('p') if self.app.selected_process.is_some() => {
                self.process_message = match &mut self.app.selected_process {
                    Some(p) => Some(p.set_priority(0)),
                    None => None,
                };
            }
            Key::Tab => {
                self.advance_to_next_section();
            }
            Key::Char('m') => {
                self.set_section_height(-2).await;
            }
            Key::Char('e') => {
                self.set_section_height(2).await;
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
            Key::Char('f') => {
                self.show_find = true;
                self.highlighted_row = 0;
                self.process_table_row_start = 0;
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
