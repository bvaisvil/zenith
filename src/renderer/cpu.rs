/**
 * Copyright 2019-2022, Benjamin Vaisvil and the zenith contributors
 */
use super::style::{max_style, ok_style, MAX_COLOR, OK_COLOR};
use crate::float_to_byte_string;
use crate::metrics::histogram::{HistogramKind, View};
use crate::metrics::zprocess::ZProcess;
use crate::metrics::{CPUTimeApp, Sensor};
use crate::renderer::{percent_of, split_left_right_pane, Render};
use byte_unit::{Byte, Unit};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{BarChart, Block, Borders, Paragraph, Sparkline, Wrap};
use ratatui::Frame;

/// Trait for abstracting CPU and memory data access, enabling mock implementations for testing.
pub trait CpuRenderData {
    fn cpu_utilization(&self) -> u64;
    fn cpus(&self) -> &[(String, u64)];
    fn frequency(&self) -> u64;
    fn cum_cpu_process(&self) -> Option<&ZProcess>;
    fn sensors(&self) -> &[Sensor];
    fn mem_utilization(&self) -> u64;
    fn mem_total(&self) -> u64;
    fn swap_utilization(&self) -> u64;
    fn swap_total(&self) -> u64;
    fn top_mem_pid(&self) -> Option<i32>;
    fn get_process(&self, pid: i32) -> Option<&ZProcess>;
}

impl CpuRenderData for CPUTimeApp {
    fn cpu_utilization(&self) -> u64 {
        self.cpu_utilization
    }
    fn cpus(&self) -> &[(String, u64)] {
        &self.cpus
    }
    fn frequency(&self) -> u64 {
        self.frequency
    }
    fn cum_cpu_process(&self) -> Option<&ZProcess> {
        self.cum_cpu_process.as_ref()
    }
    fn sensors(&self) -> &[Sensor] {
        &self.sensors
    }
    fn mem_utilization(&self) -> u64 {
        self.mem_utilization
    }
    fn mem_total(&self) -> u64 {
        self.mem_total
    }
    fn swap_utilization(&self) -> u64 {
        self.swap_utilization
    }
    fn swap_total(&self) -> u64 {
        self.swap_total
    }
    fn top_mem_pid(&self) -> Option<i32> {
        self.top_pids.mem.pid
    }
    fn get_process(&self, pid: i32) -> Option<&ZProcess> {
        self.process_map.get(&pid)
    }
}

fn cpu_title<'a, T: CpuRenderData>(app: &'a T, histogram: &'a [u64]) -> Line<'a> {
    let top_process_name = match app.cum_cpu_process() {
        Some(p) => p.name.as_str(),
        None => "",
    };
    let top_process_amt = match app.cum_cpu_process() {
        Some(p) => p.user_name.as_str(),
        None => "",
    };
    let top_pid = match app.cum_cpu_process() {
        Some(p) => p.pid,
        None => 0,
    };
    let mean: f64 = match histogram.len() {
        0 => 0.0,
        _ => histogram.iter().sum::<u64>() as f64 / histogram.len() as f64,
    };

    let peak: u64 = match histogram.len() {
        0 => 0,
        _ => histogram.iter().max().unwrap_or(&0).to_owned(),
    };
    let sensors = app.sensors();
    let temp = if !sensors.is_empty() {
        let t = sensors
            .iter()
            .map(|s| format!("{: >3.0}", s.current_temp))
            .collect::<Vec<String>>()
            .join(",");

        let hot_threshold = 70_f64;
        let cold_threshold = 40_f64;
        let numbers_txt = format!("{t:}°C");
        let max_temp = sensors
            .iter()
            .map(|s| s.current_temp as f64)
            .fold(f64::MIN, f64::max);

        if max_temp > hot_threshold {
            Span::styled(numbers_txt, max_style())
        } else if max_temp < cold_threshold {
            Span::styled(numbers_txt, Style::default().fg(Color::Cyan))
        } else {
            Span::raw(numbers_txt)
        }
    } else {
        Span::raw(String::from(""))
    };
    let cpu_utilization = app.cpu_utilization();
    Line::from(vec![
        Span::raw("CPU ["),
        Span::styled(
            format!("{: >3}%", cpu_utilization),
            if cpu_utilization > 90 {
                max_style()
            } else {
                ok_style()
            },
        ),
        Span::raw("]"),
        Span::raw(" TEMP ["),
        temp,
        Span::raw("]"),
        Span::raw(" MEAN ["),
        Span::styled(
            format!("{mean: >3.2}%",),
            if mean > 90.0 { max_style() } else { ok_style() },
        ),
        Span::raw("] PEAK ["),
        Span::styled(
            format!("{peak: >3.2}%",),
            if peak > 90 { max_style() } else { ok_style() },
        ),
        Span::raw(format!(
            "] TOP [{top_pid} - {top_process_name} - {top_process_amt}]"
        )),
    ])
}

fn mem_title<'a, T: CpuRenderData>(app: &'a T) -> Line<'a> {
    let mem_utilization = app.mem_utilization();
    let mem_total = app.mem_total();
    let swap_utilization = app.swap_utilization();
    let swap_total = app.swap_total();

    let mem = percent_of(mem_utilization, mem_total) as u64;
    let swp = percent_of(swap_utilization, swap_total) as u64;

    let top_mem_proc = match app.top_mem_pid() {
        Some(pid) => match app.get_process(pid) {
            Some(p) => format!("TOP [{:} - {:} - {:}]", p.pid, p.name, p.user_name),
            None => String::from(""),
        },
        None => String::from(""),
    };

    Line::from(vec![
        Span::raw("MEM ["),
        Span::styled(
            format!(
                "{} / {} - {:}%",
                float_to_byte_string!(mem_utilization as f64, Unit::B),
                float_to_byte_string!(mem_total as f64, Unit::B),
                mem
            ),
            if mem > 95 { max_style() } else { ok_style() },
        ),
        Span::raw("] SWP ["),
        Span::styled(
            format!(
                "{} / {} - {:}%",
                float_to_byte_string!(swap_utilization as f64, Unit::B),
                float_to_byte_string!(swap_total as f64, Unit::B),
                swp,
            ),
            if swp > 20 { max_style() } else { ok_style() },
        ),
        Span::raw("] "),
        Span::raw(top_mem_proc),
    ])
}

fn render_cpu_histogram(app: &CPUTimeApp, area: Rect, f: &mut Frame<'_>, view: &View) {
    let h = match app.histogram_map.get_zoomed(&HistogramKind::Cpu, view) {
        Some(h) => h,
        None => return,
    };
    let title = cpu_title(app, h.data());
    Sparkline::default()
        .block(Block::default().title(title))
        .data(h.data())
        .style(Style::default().fg(Color::Blue))
        .max(100)
        .render(f, area);
}

fn render_memory_histogram(app: &CPUTimeApp, area: Rect, f: &mut Frame<'_>, view: &View) {
    let h = match app.histogram_map.get_zoomed(&HistogramKind::Mem, view) {
        Some(h) => h,
        None => return,
    };
    let title2 = mem_title(app);
    Sparkline::default()
        .block(Block::default().title(title2))
        .data(h.data())
        .style(Style::default().fg(Color::Cyan))
        .max(100)
        .render(f, area);
}

fn render_cpu_bars<T: CpuRenderData>(app: &T, area: Rect, f: &mut Frame<'_>, style: &Style) {
    let cpus = app.cpus().to_owned();
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
                app.frequency()
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

        let nrows = (cpus.len() as u16).div_ceil(cols) as usize;

        let mut items = vec![];
        for i in 0..nrows {
            cpus.iter()
                .skip(i)
                .step_by(nrows)
                .take(cols.into())
                .for_each(|(label, load)| {
                    items.push(Span::raw(format!("{label:<2} ")));
                    let color = if *load < 90 { OK_COLOR } else { MAX_COLOR };
                    items.push(Span::styled(
                        format!("{load:3}"),
                        Style::default().fg(color),
                    ));
                    items.push(Span::raw("% "));
                });
        }

        Paragraph::new(Line::from(items))
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
    f: &mut Frame<'_>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use sysinfo::ProcessStatus;

    struct MockCpuData {
        cpu_utilization: u64,
        mem_utilization: u64,
        mem_total: u64,
        swap_utilization: u64,
        swap_total: u64,
        cum_cpu_process: Option<ZProcess>,
        sensors: Vec<Sensor>,
        cpus: Vec<(String, u64)>,
        frequency: u64,
        top_mem_pid: Option<i32>,
        process_map: HashMap<i32, ZProcess>,
    }

    impl Default for MockCpuData {
        fn default() -> Self {
            Self {
                cpu_utilization: 50,
                mem_utilization: 4_000_000_000,
                mem_total: 16_000_000_000,
                swap_utilization: 0,
                swap_total: 8_000_000_000,
                cum_cpu_process: None,
                sensors: vec![],
                cpus: vec![("0".to_string(), 50), ("1".to_string(), 60)],
                frequency: 3200,
                top_mem_pid: None,
                process_map: HashMap::new(),
            }
        }
    }

    impl CpuRenderData for MockCpuData {
        fn cpu_utilization(&self) -> u64 {
            self.cpu_utilization
        }
        fn cpus(&self) -> &[(String, u64)] {
            &self.cpus
        }
        fn frequency(&self) -> u64 {
            self.frequency
        }
        fn cum_cpu_process(&self) -> Option<&ZProcess> {
            self.cum_cpu_process.as_ref()
        }
        fn sensors(&self) -> &[Sensor] {
            &self.sensors
        }
        fn mem_utilization(&self) -> u64 {
            self.mem_utilization
        }
        fn mem_total(&self) -> u64 {
            self.mem_total
        }
        fn swap_utilization(&self) -> u64 {
            self.swap_utilization
        }
        fn swap_total(&self) -> u64 {
            self.swap_total
        }
        fn top_mem_pid(&self) -> Option<i32> {
            self.top_mem_pid
        }
        fn get_process(&self, pid: i32) -> Option<&ZProcess> {
            self.process_map.get(&pid)
        }
    }

    fn create_test_process(pid: i32, name: &str, user: &str) -> ZProcess {
        ZProcess {
            pid,
            uid: 1000,
            user_name: user.to_string(),
            memory: 1000000,
            cpu_usage: 10.0,
            cum_cpu_usage: 10.0,
            command: vec![name.to_string()],
            exe: format!("/usr/bin/{}", name),
            status: ProcessStatus::Run,
            name: name.to_string(),
            priority: 20,
            nice: 0,
            virtual_memory: 2000000,
            threads_total: 1,
            read_bytes: 0,
            write_bytes: 0,
            prev_read_bytes: 0,
            prev_write_bytes: 0,
            last_updated: SystemTime::now(),
            end_time: None,
            start_time: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            gpu_usage: 0,
            fb_utilization: 0,
            enc_utilization: 0,
            dec_utilization: 0,
            sm_utilization: 0,
            io_delay: Duration::from_nanos(0),
            swap_delay: Duration::from_nanos(0),
            prev_io_delay: Duration::from_nanos(0),
            prev_swap_delay: Duration::from_nanos(0),
        }
    }

    fn create_test_sensor(temp: f32) -> Sensor {
        Sensor {
            name: "CPU".to_string(),
            current_temp: temp,
            critical: 100.0,
            high: 90.0,
        }
    }

    // ==================== cpu_title tests ====================

    #[test]
    fn test_cpu_title_normal_utilization() {
        let mock = MockCpuData {
            cpu_utilization: 50,
            ..Default::default()
        };
        let histogram: Vec<u64> = vec![40, 50, 60];
        let line = cpu_title(&mock, &histogram);

        // Check that the line contains expected text
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("CPU ["));
        assert!(text.contains(" 50%"));
    }

    #[test]
    fn test_cpu_title_high_utilization() {
        let mock = MockCpuData {
            cpu_utilization: 95,
            ..Default::default()
        };
        let histogram: Vec<u64> = vec![90, 95, 92];
        let line = cpu_title(&mock, &histogram);

        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains(" 95%"));
    }

    #[test]
    fn test_cpu_title_no_sensors() {
        let mock = MockCpuData::default();
        let histogram: Vec<u64> = vec![50];
        let line = cpu_title(&mock, &histogram);

        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("TEMP ["));
        // Empty temp display when no sensors
        assert!(text.contains("TEMP []"));
    }

    #[test]
    fn test_cpu_title_cold_temperature() {
        let mock = MockCpuData {
            sensors: vec![create_test_sensor(35.0)],
            ..Default::default()
        };
        let histogram: Vec<u64> = vec![50];
        let line = cpu_title(&mock, &histogram);

        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("35°C"));
    }

    #[test]
    fn test_cpu_title_hot_temperature() {
        let mock = MockCpuData {
            sensors: vec![create_test_sensor(80.0)],
            ..Default::default()
        };
        let histogram: Vec<u64> = vec![50];
        let line = cpu_title(&mock, &histogram);

        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("80°C"));
    }

    #[test]
    fn test_cpu_title_normal_temperature() {
        let mock = MockCpuData {
            sensors: vec![create_test_sensor(55.0)],
            ..Default::default()
        };
        let histogram: Vec<u64> = vec![50];
        let line = cpu_title(&mock, &histogram);

        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("55°C"));
    }

    #[test]
    fn test_cpu_title_with_top_process() {
        let mock = MockCpuData {
            cum_cpu_process: Some(create_test_process(1234, "firefox", "testuser")),
            ..Default::default()
        };
        let histogram: Vec<u64> = vec![50];
        let line = cpu_title(&mock, &histogram);

        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("1234"));
        assert!(text.contains("firefox"));
        assert!(text.contains("testuser"));
    }

    #[test]
    fn test_cpu_title_without_top_process() {
        let mock = MockCpuData {
            cum_cpu_process: None,
            ..Default::default()
        };
        let histogram: Vec<u64> = vec![50];
        let line = cpu_title(&mock, &histogram);

        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("TOP [0 -  - ]"));
    }

    #[test]
    fn test_cpu_title_histogram_mean() {
        let mock = MockCpuData::default();
        let histogram: Vec<u64> = vec![20, 40, 60, 80];
        let line = cpu_title(&mock, &histogram);

        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        // Mean of 20,40,60,80 = 50
        assert!(text.contains("MEAN ["));
        assert!(text.contains("50.00%"));
    }

    #[test]
    fn test_cpu_title_histogram_peak() {
        let mock = MockCpuData::default();
        let histogram: Vec<u64> = vec![20, 40, 85, 60];
        let line = cpu_title(&mock, &histogram);

        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("PEAK ["));
        // peak is u64, so formatted without decimals
        assert!(text.contains("85%"));
    }

    #[test]
    fn test_cpu_title_empty_histogram() {
        let mock = MockCpuData::default();
        let histogram: Vec<u64> = vec![];
        let line = cpu_title(&mock, &histogram);

        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        // Mean and peak should be 0 for empty histogram
        assert!(text.contains("MEAN ["));
        assert!(text.contains("0.00%"));
    }

    // ==================== mem_title tests ====================

    #[test]
    fn test_mem_title_normal_memory() {
        let mock = MockCpuData {
            mem_utilization: 8_000_000_000, // 8GB
            mem_total: 16_000_000_000,      // 16GB = 50%
            ..Default::default()
        };
        let line = mem_title(&mock);

        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("MEM ["));
        assert!(text.contains("50%"));
    }

    #[test]
    fn test_mem_title_high_memory() {
        let mock = MockCpuData {
            mem_utilization: 15_500_000_000, // 15.5GB
            mem_total: 16_000_000_000,       // 16GB = ~97%
            ..Default::default()
        };
        let line = mem_title(&mock);

        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("MEM ["));
        assert!(text.contains("96%") || text.contains("97%"));
    }

    #[test]
    fn test_mem_title_normal_swap() {
        let mock = MockCpuData {
            swap_utilization: 500_000_000, // 0.5GB
            swap_total: 8_000_000_000,     // 8GB = ~6%
            ..Default::default()
        };
        let line = mem_title(&mock);

        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("SWP ["));
    }

    #[test]
    fn test_mem_title_high_swap() {
        let mock = MockCpuData {
            swap_utilization: 2_000_000_000, // 2GB
            swap_total: 8_000_000_000,       // 8GB = 25%
            ..Default::default()
        };
        let line = mem_title(&mock);

        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("SWP ["));
        assert!(text.contains("25%"));
    }

    #[test]
    fn test_mem_title_with_top_process() {
        let proc = create_test_process(5678, "chrome", "testuser");
        let mut process_map = HashMap::new();
        process_map.insert(5678, proc);

        let mock = MockCpuData {
            top_mem_pid: Some(5678),
            process_map,
            ..Default::default()
        };
        let line = mem_title(&mock);

        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("TOP [5678"));
        assert!(text.contains("chrome"));
    }

    #[test]
    fn test_mem_title_without_top_process() {
        let mock = MockCpuData {
            top_mem_pid: None,
            ..Default::default()
        };
        let line = mem_title(&mock);

        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        // Should not contain TOP when no process
        assert!(!text.contains("TOP ["));
    }

    #[test]
    fn test_mem_title_top_process_not_in_map() {
        let mock = MockCpuData {
            top_mem_pid: Some(9999),
            process_map: HashMap::new(), // Process not in map
            ..Default::default()
        };
        let line = mem_title(&mock);

        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        // Should not show TOP if process not found
        assert!(!text.contains("TOP [9999"));
    }
}
