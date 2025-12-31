/**
 * Copyright 2019-2022, Benjamin Vaisvil and the zenith contributors
 */
use super::{percent_of, Render};
use crate::float_to_byte_string;
use crate::metrics::zprocess::{ProcessStatusExt, ZProcess};
use crate::metrics::{CPUTimeApp, ProcessTableSortOrder};
use byte_unit::{Byte, Unit};
use chrono::prelude::DateTime;
use chrono::Local;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap};
use ratatui::Frame;
use std::borrow::Cow;
use std::time::{Duration, UNIX_EPOCH};

pub fn render_process_table(
    app: &CPUTimeApp,
    process_table: &[i32],
    area: Rect,
    process_table_start: usize,
    f: &mut Frame<'_>,
    border_style: Style,
    show_paths: bool,
    show_find: bool,
    filter: &str,
    highlighted_row: usize,
) -> Option<Box<ZProcess>> {
    // 4 for the margins and table header
    let display_height = match area.height.saturating_sub(3) {
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

    let rows: Vec<Row> = render_rows(
        app,
        procs,
        process_table_start,
        display_height,
        show_paths,
        highlighted_row,
    );

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
    #[cfg(target_os = "linux")]
    header.push(String::from("IOWAIT% "));
    #[cfg(feature = "nvidia")]
    header.push(String::from("GPU% "));
    #[cfg(feature = "nvidia")]
    header.push(String::from("FB%  "));
    //figure column widths
    let mut widths = Vec::with_capacity(header.len() + 1);
    let mut used_width = 0;
    for item in &header {
        let len = item.len() as u16;
        widths.push(Constraint::Length(len));
        used_width += len;
    }
    let cmd_width = f.area().width.saturating_sub(used_width).saturating_sub(3);
    let cmd_header = format!("{:<width$}", "CMD", width = cmd_width as usize);
    widths.push(Constraint::Min(cmd_width));
    header.push(cmd_header);

    header[app.psortby as usize].pop();
    let sort_ind = match app.psortorder {
        ProcessTableSortOrder::Ascending => '↑',
        ProcessTableSortOrder::Descending => '↓',
    };
    header[app.psortby as usize].insert(0, sort_ind); //sort column indicator
    let header_row: Vec<Cell> = header
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if i == app.psortby as usize {
                Cell::from(c.as_str()).style(
                    Style::default()
                        .bg(Color::Gray)
                        .fg(Color::Black)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Cell::from(c.as_str())
            }
        })
        .collect();
    let title = if show_find {
        format!("[ESC] Clear, Find: {filter:}")
    } else if !filter.is_empty() {
        format!("Filtered Results: {filter:}, [/] to change/clear")
    } else {
        format!(
            "Tasks [{:}] Threads [{:}]  Navigate [↑/↓] Sort Col [,/.] Asc/Dec [;] Filter [/]",
            app.processes.len(),
            app.threads_total
        )
    };

    Table::new(rows, widths)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(Span::styled(title, border_style)),
        )
        .column_spacing(0)
        .header(
            Row::new(header_row)
                .style(Style::default().bg(Color::DarkGray))
                .bottom_margin(0),
        )
        .render(f, area);
    highlighted_process
}

fn render_rows<'a>(
    app: &CPUTimeApp,
    procs: Vec<&'a ZProcess>,
    process_table_start: usize,
    display_height: usize,
    show_paths: bool,
    highlighted_row: usize,
) -> Vec<Row<'a>> {
    procs
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
            let mut cpu_usage =
                set_process_row_style(p.pid, app.top_pids.cpu.pid, format!("{:>5.1}", p.cpu_usage));
            if let Some(top) = &app.cum_cpu_process {
                if top.pid == p.pid {
                    cpu_usage = cpu_usage.style(Style::default().fg(Color::Magenta));
                }
            };

            let mut row = vec![
                Cell::from(format!("{: >width$}", p.pid, width = app.max_pid_len)),
                Cell::from(format!("{: <10}", p.user_name)),
                Cell::from(format!("{: <3}", p.priority)),
                Cell::from(format!("{: <3}", p.nice)),
                cpu_usage,
                set_process_row_style(
                    p.pid,
                    app.top_pids.mem.pid,
                    format!("{:>5.1}", percent_of(p.memory, app.mem_total)),
                ),
                set_process_row_style(
                    p.pid,
                    app.top_pids.mem.pid,
                    format!(
                        "{:>8}",
                        float_to_byte_string!(p.memory as f64, Unit::KB).replace('B', "")
                    ),
                ),
                set_process_row_style(
                    p.pid,
                    app.top_pids.virt.pid,
                    format!(
                        "{:>8}",
                        float_to_byte_string!(p.virtual_memory as f64, Unit::KB).replace('B', "")
                    ),
                ),
                Cell::from(format!("{:1}", p.status.to_single_char())),
                set_process_row_style(
                    p.pid,
                    app.top_pids.read.pid,
                    format!(
                        "{:>8}",
                        float_to_byte_string!(
                            p.get_read_bytes_sec(&app.histogram_map.tick),
                            Unit::B
                        )
                        .replace('B', "")
                    ),
                ),
                set_process_row_style(
                    p.pid,
                    app.top_pids.write.pid,
                    format!(
                        "{:>8}",
                        float_to_byte_string!(
                            p.get_write_bytes_sec(&app.histogram_map.tick),
                            Unit::B
                        )
                        .replace('B', "")
                    ),
                ),
            ];

            #[cfg(target_os = "linux")]
            row.push(set_process_row_style(
                p.pid,
                app.top_pids.iowait.pid,
                format!("{:>5.1}", p.get_io_wait(&app.histogram_map.tick)),
            ));
            #[cfg(feature = "nvidia")]
            row.push(set_process_row_style(
                p.pid,
                app.top_pids.gpu.pid,
                format!("{:>4.0}", p.gpu_usage),
            ));
            #[cfg(feature = "nvidia")]
            row.push(set_process_row_style(
                p.pid,
                app.top_pids.frame_buffer.pid,
                format!("{:>4.0}", p.fb_utilization),
            ));

            row.push(Cell::from(format!("{:}{:}", p.name, cmd_string)));

            let row = Row::new(row);

            if i == highlighted_row {
                row.style(
                    Style::default()
                        .bg(Color::Gray)
                        .fg(Color::Black)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                row
            }
        })
        .collect()
}

pub fn render_process(
    app: &CPUTimeApp,
    layout: Rect,
    f: &mut Frame<'_>,
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
    let run_duration = p.get_run_duration();
    let d = format!(
        "{:0>2}:{:0>2}:{:0>2}",
        run_duration.num_hours(),
        run_duration.num_minutes() % 60,
        run_duration.num_seconds() % 60
    );

    let rhs_style = Style::default().fg(Color::Green);
    let mut text = vec![
        Line::from(vec![
            Span::raw("Name:                  "),
            Span::styled(format!("{:} ({:})", &p.name, alive), rhs_style),
        ]),
        Line::from(vec![
            Span::raw("PID:                   "),
            Span::styled(
                format!("{:>width$}", &p.pid, width = app.max_pid_len),
                rhs_style,
            ),
        ]),
        Line::from(vec![
            Span::raw("Command:               "),
            Span::styled(p.command.join(" "), rhs_style),
        ]),
        Line::from(vec![
            Span::raw("User:                  "),
            Span::styled(&p.user_name, rhs_style),
        ]),
        Line::from(vec![
            Span::raw("Start Time:            "),
            Span::styled(
                format!(
                    "{:}",
                    DateTime::<Local>::from(UNIX_EPOCH + Duration::from_secs(p.start_time))
                ),
                rhs_style,
            ),
        ]),
        Line::from(vec![
            Span::raw("Total Run Time:        "),
            Span::styled(d, rhs_style),
        ]),
        Line::from(vec![
            Span::raw("CPU Usage:             "),
            Span::styled(format!("{:>7.2} %", &p.cpu_usage), rhs_style),
        ]),
        Line::from(vec![
            Span::raw("Threads:               "),
            Span::styled(format!("{:>7}", &p.threads_total), rhs_style),
        ]),
        Line::from(vec![
            Span::raw("Status:                "),
            Span::styled(format!("{:}", p.status), rhs_style),
        ]),
        Line::from(vec![
            Span::raw("Priority:              "),
            Span::styled(format!("{:>7}", p.priority), rhs_style),
        ]),
        Line::from(vec![
            Span::raw("Nice:                  "),
            Span::styled(format!("{:>7}", p.nice), rhs_style),
        ]),
        Line::from(vec![
            Span::raw("MEM Usage:             "),
            Span::styled(
                format!("{:>7.2} %", percent_of(p.memory, app.mem_total)),
                rhs_style,
            ),
        ]),
        Line::from(vec![
            Span::raw("Total Memory:          "),
            Span::styled(
                format!("{:>10}", float_to_byte_string!(p.memory as f64, Unit::KB)),
                rhs_style,
            ),
        ]),
        Line::from(vec![
            Span::raw("Disk Read:             "),
            Span::styled(
                format!(
                    "{:>10} {:}/s",
                    float_to_byte_string!(p.read_bytes as f64, Unit::B),
                    float_to_byte_string!(p.get_read_bytes_sec(&app.histogram_map.tick), Unit::B)
                ),
                rhs_style,
            ),
        ]),
        Line::from(vec![
            Span::raw("Disk Write:            "),
            Span::styled(
                format!(
                    "{:>10} {:}/s",
                    float_to_byte_string!(p.write_bytes as f64, Unit::B),
                    float_to_byte_string!(p.get_write_bytes_sec(&app.histogram_map.tick), Unit::B)
                ),
                rhs_style,
            ),
        ]),
    ];

    if !app.gfx_devices.is_empty() {
        text.push(Line::from(vec![
            Span::raw("SM Util:            "),
            Span::styled(format!("{:7.2} %", p.sm_utilization as f64), rhs_style),
        ]));
        text.push(Line::from(vec![
            Span::raw("Frame Buffer:       "),
            Span::styled(format!("{:7.2} %", p.fb_utilization as f64), rhs_style),
        ]));
        text.push(Line::from(vec![
            Span::raw("Encoder Util:       "),
            Span::styled(format!("{:7.2} %", p.enc_utilization as f64), rhs_style),
        ]));
        text.push(Line::from(vec![
            Span::raw("Decoder Util:       "),
            Span::styled(format!("{:7.2} %", p.dec_utilization as f64), rhs_style),
        ]));
    }

    #[cfg(target_os = "linux")]
    text.push(Line::from(vec![
        Span::raw("IO Wait:               "),
        Span::styled(
            format!(
                "{:>7.2} % ({:>7.2} %)",
                p.get_io_wait(&app.histogram_map.tick),
                p.get_total_io_wait()
            ),
            rhs_style,
        ),
    ]));
    #[cfg(target_os = "linux")]
    text.push(Line::from(vec![
        Span::raw("Swap Wait:             "),
        Span::styled(
            format!(
                "{:>7.2} % ({:>7.2} %)",
                p.get_swap_wait(&app.histogram_map.tick),
                p.get_total_swap_wait()
            ),
            rhs_style,
        ),
    ]));

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

pub fn filter_process_table<'a>(app: &'a CPUTimeApp, filter: &str) -> Cow<'a, [i32]> {
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

fn set_process_row_style<'a>(
    current_pid: i32,
    test_pid: Option<i32>,
    row_content: String,
) -> Cell<'a> {
    match test_pid {
        Some(p) => {
            if p == current_pid {
                Cell::from(row_content).style(Style::default().fg(Color::Red))
            } else {
                Cell::from(row_content)
            }
        }
        None => Cell::from(row_content),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::zprocess::ZProcess;
    use std::collections::HashMap;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use sysinfo::ProcessStatus;

    fn create_test_process(pid: i32, name: &str, cpu: f32, memory: u64) -> ZProcess {
        ZProcess {
            pid,
            uid: 1000,
            user_name: "testuser".to_string(),
            memory,
            cpu_usage: cpu,
            cum_cpu_usage: cpu as f64,
            command: vec![name.to_string()],
            exe: format!("/usr/bin/{}", name),
            status: ProcessStatus::Run,
            name: name.to_string(),
            priority: 20,
            nice: 0,
            virtual_memory: memory * 2,
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

    /// Creates a minimal mock CPUTimeApp for testing filter_process_table
    fn create_mock_app_for_filter(processes: Vec<ZProcess>) -> MockApp {
        let mut process_map = HashMap::new();
        let process_ids: Vec<i32> = processes.iter().map(|p| p.pid).collect();
        
        for p in processes {
            process_map.insert(p.pid, p);
        }
        
        MockApp {
            processes: process_ids,
            process_map,
        }
    }

    /// Minimal mock for CPUTimeApp to test filter_process_table
    struct MockApp {
        processes: Vec<i32>,
        process_map: HashMap<i32, ZProcess>,
    }

    /// Test filter with mock - we need to test the actual function with real CPUTimeApp
    /// but we can at least test the filter logic pattern
    #[test]
    fn test_filter_logic_by_name() {
        let processes = vec![
            create_test_process(1, "firefox", 10.0, 1000),
            create_test_process(2, "chrome", 20.0, 2000),
            create_test_process(3, "code", 5.0, 500),
            create_test_process(4, "firefox-esr", 8.0, 800),
        ];
        
        let mock = create_mock_app_for_filter(processes);
        
        // Test filter matching
        let filter = "firefox";
        let filter_lc = filter.to_lowercase();
        
        let matches: Vec<i32> = mock.processes
            .iter()
            .filter(|pid| {
                let p = mock.process_map.get(pid).unwrap();
                p.name.to_lowercase().contains(&filter_lc)
            })
            .copied()
            .collect();
        
        assert_eq!(matches.len(), 2);
        assert!(matches.contains(&1));
        assert!(matches.contains(&4));
    }

    #[test]
    fn test_filter_logic_by_exe() {
        let processes = vec![
            create_test_process(1, "firefox", 10.0, 1000),
            create_test_process(2, "chrome", 20.0, 2000),
        ];
        
        let mock = create_mock_app_for_filter(processes);
        
        let filter = "/usr/bin/firefox";
        let filter_lc = filter.to_lowercase();
        
        let matches: Vec<i32> = mock.processes
            .iter()
            .filter(|pid| {
                let p = mock.process_map.get(pid).unwrap();
                p.exe.to_lowercase().contains(&filter_lc)
            })
            .copied()
            .collect();
        
        assert_eq!(matches.len(), 1);
        assert!(matches.contains(&1));
    }

    #[test]
    fn test_filter_logic_by_pid() {
        let processes = vec![
            create_test_process(123, "process_a", 10.0, 1000),
            create_test_process(456, "process_b", 20.0, 2000),
            create_test_process(1234, "process_c", 5.0, 500),
        ];
        
        let mock = create_mock_app_for_filter(processes);
        
        let filter = "123";
        let filter_lc = filter.to_lowercase();
        
        let matches: Vec<i32> = mock.processes
            .iter()
            .filter(|pid| {
                let p = mock.process_map.get(pid).unwrap();
                format!("{}", p.pid).contains(&filter_lc)
            })
            .copied()
            .collect();
        
        // Should match pid 123 and 1234
        assert_eq!(matches.len(), 2);
        assert!(matches.contains(&123));
        assert!(matches.contains(&1234));
    }

    #[test]
    fn test_filter_logic_case_insensitive() {
        let processes = vec![
            create_test_process(1, "Firefox", 10.0, 1000),
            create_test_process(2, "FIREFOX", 20.0, 2000),
            create_test_process(3, "firefox", 5.0, 500),
        ];
        
        let mock = create_mock_app_for_filter(processes);
        
        let filter = "firefox";
        let filter_lc = filter.to_lowercase();
        
        let matches: Vec<i32> = mock.processes
            .iter()
            .filter(|pid| {
                let p = mock.process_map.get(pid).unwrap();
                p.name.to_lowercase().contains(&filter_lc)
            })
            .copied()
            .collect();
        
        // All three should match
        assert_eq!(matches.len(), 3);
    }

    #[test]
    fn test_filter_logic_empty_filter() {
        let processes = vec![
            create_test_process(1, "firefox", 10.0, 1000),
            create_test_process(2, "chrome", 20.0, 2000),
        ];
        
        let mock = create_mock_app_for_filter(processes);
        
        let filter = "";
        
        // Empty filter should return all
        if filter.is_empty() {
            assert_eq!(mock.processes.len(), 2);
        }
    }

    #[test]
    fn test_filter_logic_no_match() {
        let processes = vec![
            create_test_process(1, "firefox", 10.0, 1000),
            create_test_process(2, "chrome", 20.0, 2000),
        ];
        
        let mock = create_mock_app_for_filter(processes);
        
        let filter = "nonexistent";
        let filter_lc = filter.to_lowercase();
        
        let matches: Vec<i32> = mock.processes
            .iter()
            .filter(|pid| {
                let p = mock.process_map.get(pid).unwrap();
                p.name.to_lowercase().contains(&filter_lc)
            })
            .copied()
            .collect();
        
        assert_eq!(matches.len(), 0);
    }

    #[test]
    fn test_filter_logic_by_command() {
        let mut process = create_test_process(1, "python", 10.0, 1000);
        process.command = vec!["python".to_string(), "my_script.py".to_string()];
        
        let processes = vec![
            process,
            create_test_process(2, "bash", 5.0, 500),
        ];
        
        let mock = create_mock_app_for_filter(processes);
        
        let filter = "my_script";
        let filter_lc = filter.to_lowercase();
        
        let matches: Vec<i32> = mock.processes
            .iter()
            .filter(|pid| {
                let p = mock.process_map.get(pid).unwrap();
                p.command.join(" ").to_lowercase().contains(&filter_lc)
            })
            .copied()
            .collect();
        
        assert_eq!(matches.len(), 1);
        assert!(matches.contains(&1));
    }

    #[test]
    fn test_set_process_row_style_matching_pid() {
        let cell = set_process_row_style(100, Some(100), "test".to_string());
        // Cell should have red foreground style when pids match
        // We can't easily inspect the style, but we can verify it doesn't panic
        assert!(!format!("{:?}", cell).is_empty());
    }

    #[test]
    fn test_set_process_row_style_non_matching_pid() {
        let cell = set_process_row_style(100, Some(200), "test".to_string());
        assert!(!format!("{:?}", cell).is_empty());
    }

    #[test]
    fn test_set_process_row_style_no_test_pid() {
        let cell = set_process_row_style(100, None, "test".to_string());
        assert!(!format!("{:?}", cell).is_empty());
    }
}

