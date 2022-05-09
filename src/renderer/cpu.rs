/**
 * Copyright 2019-2022, Benjamin Vaisvil and the zenith contributors
 */
use crate::float_to_byte_string;
use crate::metrics::histogram::{HistogramKind, View};
use crate::metrics::CPUTimeApp;
use crate::renderer::{percent_of, split_left_right_pane, Render, ZBackend};
use byte_unit::{Byte, ByteUnit};
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::{Color, Modifier, Style};
use tui::text::{Span, Spans};
use tui::widgets::{BarChart, Block, Borders, Paragraph, Sparkline, Wrap};
use tui::Frame;

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

fn mem_title(app: &CPUTimeApp) -> String {
    let mem = percent_of(app.mem_utilization, app.mem_total) as u64;
    let swp = percent_of(app.swap_utilization, app.swap_total) as u64;

    let top_mem_proc = match app.top_pids.mem.pid {
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

pub fn render_cpu(
    app: &CPUTimeApp,
    area: Rect,
    f: &mut Frame<'_, ZBackend>,
    view: View,
    border_style: Style,
) {
    let (cpu_layout, view) = split_left_right_pane("", area, f, view, border_style);

    let cpu_mem = Layout::default()
        .margin(1)
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(cpu_layout[1]);
    render_cpu_histogram(app, cpu_mem[0], f, &view);
    render_memory_histogram(app, cpu_mem[1], f, &view);
    render_cpu_bars(app, cpu_layout[0], f, &border_style);
}
