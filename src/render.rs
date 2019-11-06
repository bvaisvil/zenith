/**
 * Copyright 2019 Benjamin Vaisvil (ben@neuon.com)
 */
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::{Color, Modifier, Style};
use sysinfo::{DiskExt};
use tui::layout::Corner;
use tui::widgets::{BarChart, Block, Borders, Widget, Sparkline, Paragraph, Text, Table, Row, List};
use byte_unit::{Byte, ByteUnit};
use termion::event::Key;
use termion::input::MouseTerminal;
use termion::raw::{IntoRawMode, RawTerminal};
use termion::screen::AlternateScreen;
use tui::backend::TermionBackend;
use tui::Terminal;
use std::io::{Write, Stdout};
use tui::Frame;
use hostname::get_hostname;
use std::io;
use crate::util::*;
use crate::metrics::*;
use crate::zprocess::*;
use std::ffi::{OsStr, OsString};
use std::borrow::Cow;

fn mem_title(app: &CPUTimeApp) -> String {
    let mut mem: u64 = 0;
    if app.mem_utilization > 0 && app.mem_total > 0 {
        mem = ((app.mem_utilization as f32 / app.mem_total as f32) * 100.0) as u64;
    }
    let mut swp: u64 = 0;
    if app.swap_utilization > 0 && app.swap_total > 0 {
        swp = ((app.swap_utilization as f32 / app.swap_total as f32) * 100.0) as u64;
    }
    format!("MEM [{}] Usage [{: >3}%] SWP [{}] Usage [{: >3}%]",
            Byte::from_unit(app.mem_total as f64, ByteUnit::KB).unwrap()
                .get_appropriate_unit(false).to_string().replace(" ", ""),
            mem,
            Byte::from_unit(app.swap_total as f64, ByteUnit::KB).unwrap()
                .get_appropriate_unit(false).to_string().replace(" ", ""),
            swp
    )
}

fn cpu_title(app: &CPUTimeApp) -> String {
    let top_process_name = match app.cum_cpu_process {
        Some(pid) => match &app.process_map.get(&pid){
            Some(zp) => &zp.name,
            None => ""
        }
        None => ""
    };
    let top_process_amt = match app.cum_cpu_process {
        Some(pid) => match &app.process_map.get(&pid){
            Some(zp) => zp.user_name.clone(),
            None => String::from("")
        }
        None => String::from("")
    };
    let top_pid = app.cum_cpu_process.unwrap_or(0);
    let mean: u64 = match app.cpu_usage_histogram.len(){
        0 => 0,
        _ => app.cpu_usage_histogram.iter().sum::<u64>() / app.cpu_usage_histogram.len() as u64,
    };
    format!("CPU [{: >3}%] MEAN [{: >3}%] TOP [{} - {} - {}]",
            app.cpu_utilization,
            mean,
            top_pid,
            top_process_name,
            top_process_amt
    )
}


fn render_process_table<'a>(
    app: &CPUTimeApp,
    width: u16,
    area: Rect,
    process_table_start: usize,
    f: &mut Frame<TermionBackend<AlternateScreen<MouseTerminal<RawTerminal<Stdout>>>>>) {
    // process table
    if area.height < 5{
        return;
    }
    let display_height = area.height as usize - 4; // 4 for the margins and table header
    //panic!("{}", area.height);
    let end = process_table_start + display_height;

    let mut rows: Vec<Vec<String>> = app.processes.iter().map(|(pid)| {
        let p = app.process_map.get(pid).unwrap();
        vec![
            format!("{: >5}", p.pid),
            format!("{: <10}", p.user_name),
            format!("{: <3}", p.priority),
            format!("{:>5.1}", p.cpu_usage),
            format!("{:>5.1}", (p.memory as f64 / app.mem_utilization as f64) * 100.0),
            format!("{:>8}", Byte::from_unit(p.memory as f64, ByteUnit::KB)
                .unwrap().get_appropriate_unit(false).to_string().replace(" ", "").replace("B", "")),
            format!("{: >8}", Byte::from_unit(p.virtual_memory as f64, ByteUnit::KB)
                .unwrap().get_appropriate_unit(false).to_string().replace(" ", "").replace("B", "")),
            format!("{:1}", p.status.to_single_char()),
            format!("{:>8}", Byte::from_unit(p.get_read_bytes_sec(), ByteUnit::B).unwrap().get_appropriate_unit(false).to_string().replace(" ", "").replace("B", "")),
            format!("{:>8}", Byte::from_unit(p.get_write_bytes_sec(), ByteUnit::B).unwrap().get_appropriate_unit(false).to_string().replace(" ", "").replace("B", "")),
            format!("{} - ", p.name) + &[p.exe.as_str(), p.command.join(" ").as_str()].join(" ")
        ]
    }).collect();
    let mut header = vec![
        String::from("PID   "),
        String::from("USER       "),
        String::from("P   "),
        String::from("CPU%  "),
        String::from("MEM%  "),
        String::from("MEM     "),
        String::from("VIRT     "),
        String::from("S "),
        String::from("READ/s   "),
        String::from("WRITE/s  ")
    ];
    let mut widths: Vec<u16> = header.iter().map(|item| item.len() as u16).collect();
    let s: u16 = widths.iter().sum();
    let mut cmd_width = width as i16 - s as i16 - 3;
    if cmd_width < 0 {
        cmd_width = 0;
    }
    let cmd_width = cmd_width as u16;
    let mut cmd_header = String::from("CMD");
    for i in 3..cmd_width {
        cmd_header.push(' ');
    }
    header.push(cmd_header);
    widths.push(header.last().unwrap().len() as u16);
    header[app.psortby as usize].pop();
    let sort_ind = match app.psortorder{
        ProcessTableSortOrder::Ascending => '↑',
        ProcessTableSortOrder::Descending => '↓'
    };
    header[app.psortby as usize].insert(0,sort_ind); //sort column indicator
    let rows = rows.iter().enumerate().filter_map(|(i, r)| {
        if i >= process_table_start && i < end{
            if app.highlighted_row == i{
                Some(Row::StyledData(r.into_iter(), Style::default().fg(Color::Magenta).modifier(Modifier::BOLD)))
            }
            else{
                Some(Row::Data(r.into_iter()))
            }
        }
        else{
            None
        }
    });

    Table::new(header.into_iter(), rows)
        .block(Block::default().borders(Borders::ALL)
            .title(format!("Tasks [{}] Threads [{}]", app.processes.len(), app.threads_total).as_str()))
        .widths(widths.as_slice())
        .column_spacing(0)
        .header_style(Style::default().bg(Color::DarkGray)).render(f, area);

}

fn render_cpu_histogram(app: &CPUTimeApp, area: Rect, 
f: &mut Frame<TermionBackend<AlternateScreen<MouseTerminal<RawTerminal<Stdout>>>>>){
    let title = cpu_title(&app);
    let start_at = if app.cpu_usage_histogram.len() > area.width as usize
    {
        app.cpu_usage_histogram.len() - area.width as usize
    }
    else{
        0
    };
    Sparkline::default()
        .block(
            Block::default().title(title.as_str()))
        .data(&app.cpu_usage_histogram[start_at..])
        .style(Style::default().fg(Color::Blue))
        .max(100)
        .render(f, area);
}

fn render_memory_histogram(app: &CPUTimeApp, area: Rect, f: &mut Frame<TermionBackend<AlternateScreen<MouseTerminal<RawTerminal<Stdout>>>>>) {
    let title2 = mem_title(&app);
    let start_at = if app.mem_usage_histogram.len() > area.width as usize
    {
        app.mem_usage_histogram.len() - area.width as usize
    }
    else{
        0
    };
    Sparkline::default()
        .block(
            Block::default().title(title2.as_str()))
        .data(&app.mem_usage_histogram[start_at..])
        .style(Style::default().fg(Color::Cyan))
        .max(100)
        .render(f, area);
}

fn render_cpu_bars(app: &CPUTimeApp, area: Rect, width: u16, f: &mut Frame<TermionBackend<AlternateScreen<MouseTerminal<RawTerminal<Stdout>>>>>){

    let mut cpus = app.cpus.to_owned();
    let mut bars: Vec<(&str, u64)> = vec![];
    let mut bar_gap: u16 = 1;


    let mut np = app.cpus.len() as u16;
    if np == 0 {
        np = 1;
    }
    if width > 2 && (np * 2) >= width - 2{
        bar_gap = 0;
    }
    let mut cpu_bw = ((width as f32 - (np * bar_gap) as f32) / np as f32) as i16;
    if cpu_bw < 1 {
        cpu_bw = 1;
    }
    let cpu_bw = cpu_bw as u16;
    for (i, (p, u)) in cpus.iter_mut().enumerate() {

        if i > 8 && cpu_bw == 1{
            p.remove(0);
        }
        bars.push((p.as_str(), u.clone()));
    }

    // Bar chart for current CPU usage.
    BarChart::default()
        .block(Block::default().title(format!("CPU(S) [{}] Freq [{} MHz]", np, app.frequency).as_str()).borders(Borders::ALL))
        .data(bars.as_slice())
        .bar_width(cpu_bw)
        .bar_gap(bar_gap)
        .max(100)
        .style(Style::default().fg(Color::Green))
        .value_style(Style::default().bg(Color::Green).modifier(Modifier::BOLD))
        .render(f, area);
}

fn render_overview(app: &CPUTimeApp, area: Rect,  hostname: &str, f: &mut Frame<TermionBackend<AlternateScreen<MouseTerminal<RawTerminal<Stdout>>>>>){
    BarChart::default()
        .block(Block::default().title(hostname).title_style(Style::default().modifier(Modifier::BOLD)).borders(Borders::ALL).border_style(Style::default().fg(Color::Red)))
        .data(&app.overview)
        .style(Style::default().fg(Color::Red))
        .bar_width(3)
        .bar_gap(1)
        .max(100)
        .value_style(Style::default().bg(Color::Red))
        .label_style(Style::default().fg(Color::Cyan).modifier(Modifier::ITALIC))
        .render(f, area);
}

fn render_net(app: &CPUTimeApp, area: Vec<Rect>,
              f: &mut Frame<TermionBackend<AlternateScreen<MouseTerminal<RawTerminal<Stdout>>>>>){
    let up_max: u64 = match app.net_out_histogram.iter().max(){
        Some(x) => x.clone(),
        None => 1
    };
    let up_max_bytes =  Byte::from_unit(up_max as f64, ByteUnit::B).unwrap().get_appropriate_unit(false);

    let net_up = Byte::from_unit(app.net_out as f64, ByteUnit::B).unwrap().get_appropriate_unit(false);
    let start_at = if app.net_out_histogram.len() > area[0].width as usize
    {
        app.net_out_histogram.len() - area[0].width as usize
    }
    else{
        0
    };
    Sparkline::default()
        .block(Block::default().title(format!("↑ [{:^10}] Max [{:^10}]", net_up.to_string(), up_max_bytes.to_string()).as_str()))
        .data(&app.net_out_histogram[start_at..])
        .style(Style::default().fg(Color::LightYellow))
        .max(up_max)
        .render(f, area[0]);


    let down_max: u64 = match app.net_in_histogram.iter().max(){
        Some(x) => x.clone(),
        None => 1
    };
    let net_down = Byte::from_unit(app.net_in as f64, ByteUnit::B).unwrap().get_appropriate_unit(false);
    let down_max_bytes =  Byte::from_unit(down_max as f64, ByteUnit::B).unwrap().get_appropriate_unit(false);
    let start_at = if app.net_in_histogram.len() > area[1].width as usize
    {
        app.net_in_histogram.len() - area[1].width as usize
    }
    else{
        0
    };
    Sparkline::default()
        .block(Block::default().title(format!("↓ [{:^10}] Max [{:^10}]", net_down.to_string(), down_max_bytes.to_string()).as_str()))
        .data(&app.net_in_histogram[start_at..])
        .style(Style::default().fg(Color::LightMagenta))
        .max(down_max)
        .render(f, area[1]);
}

fn render_disk(app: &CPUTimeApp, disk_layout: Vec<Rect>,
              f: &mut Frame<TermionBackend<AlternateScreen<MouseTerminal<RawTerminal<Stdout>>>>>){
    let area = Layout::default().margin(1).direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref()).split(disk_layout[1]);
    let read_max: u64 = match app.disk_read_histogram.iter().max(){
        Some(x) => x.clone(),
        None => 1
    };
    let read_max_bytes =  Byte::from_unit(read_max as f64, ByteUnit::B).unwrap().get_appropriate_unit(false);

    let read_up = Byte::from_unit(app.disk_read as f64, ByteUnit::B).unwrap().get_appropriate_unit(false);
    let start_at = if app.disk_read_histogram.len() > area[0].width as usize
    {
        app.disk_read_histogram.len() - area[0].width as usize
    }
    else{
        0
    };
    Sparkline::default()
        .block(Block::default().title(format!("R [{:^10}] Max [{:^10}]", read_up.to_string(), read_max_bytes.to_string()).as_str()))
        .data(&app.disk_read_histogram[start_at..])
        .style(Style::default().fg(Color::LightYellow))
        .max(read_max)
        .render(f, area[0]);


    let write_max: u64 = match app.disk_write_histogram.iter().max(){
        Some(x) => x.clone(),
        None => 1
    };
    let write_down = Byte::from_unit(app.disk_write as f64, ByteUnit::B).unwrap().get_appropriate_unit(false);
    let write_max_bytes =  Byte::from_unit(write_max as f64, ByteUnit::B).unwrap().get_appropriate_unit(false);
    let start_at = if app.disk_write_histogram.len() > area[1].width as usize
    {
        app.disk_write_histogram.len() - area[1].width as usize
    }
    else{
        0
    };
    Sparkline::default()
        .block(Block::default().title(format!("W [{:^10}] Max [{:^10}]", write_down.to_string(), write_max_bytes.to_string()).as_str()))
        .data(&app.disk_write_histogram[start_at..])
        .style(Style::default().fg(Color::LightMagenta))
        .max(write_max)
        .render(f, area[1]);
    let disks = app.disks.iter().map(|d| {
        if d.get_perc_free_space() < 10.0{
                Text::Styled(
            Cow::Owned(format!("{:.2}%(!): {}", d.get_perc_free_space(),d.get_mount_point().display())),
            Style::default().fg(Color::Red).modifier(Modifier::BOLD))
        }
        else{
        Text::Styled(
            Cow::Owned(format!("{:.2}%: {}", d.get_perc_free_space(), d.get_mount_point().display())),
            Style::default().fg(Color::Green))
        }

    });
    List::new(disks).block(Block::default().title("Disks").borders(Borders::ALL)).render(f, disk_layout[0]);
}


pub struct TerminalRenderer<'a>{
    terminal: Terminal<TermionBackend<AlternateScreen<MouseTerminal<RawTerminal<Stdout>>>>>,
    app: CPUTimeApp<'a>,
    events: Events,
    hostname: String,
    process_table_row_start: usize
}

impl<'a> TerminalRenderer<'a> {
    pub fn new() -> TerminalRenderer<'a> {
        let stdout = io::stdout().into_raw_mode().expect("Could not bind to STDOUT in raw mode.");
        let stdout = MouseTerminal::from(stdout);
        let stdout = AlternateScreen::from(stdout);
        let backend = TermionBackend::new(stdout);
        TerminalRenderer {
            terminal: Terminal::new(backend).unwrap(),
            app: CPUTimeApp::new(),
            events: Events::new(),
            hostname: get_hostname().unwrap_or(String::from("")),
            process_table_row_start: 0
        }
    }

    pub fn start(&mut self) {
        loop {
            let mut app = &self.app;
            let hostname = &self.hostname;
            let pst = &self.process_table_row_start;
            let mut width: u16 = 0;
            let mut process_table_height: u16 = 0;
            self.terminal.draw( |mut f| {
                width = f.size().width;
                // primary layout division.
                let v_sections = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(0)
                    .constraints([
                        Constraint::Length(8),
                        Constraint::Length(10),
                        Constraint::Length(10),
                        Constraint::Length(10),
                        Constraint::Min(8)
                    ].as_ref())
                    .split(f.size());


                let overview_width: u16 = 18;
                let mut cpu_width: i16 = width as i16 - overview_width as i16;
                if cpu_width < 1 {
                    cpu_width = 1;
                }

                Block::default().title("CPU & Memory").borders(Borders::ALL).render(&mut f, v_sections[1]);
                let cpu_mem = Layout::default().margin(1).direction(Direction::Vertical)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref()).split(v_sections[1]);

                // secondary layout
                let h_sections = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Length(overview_width),
                        Constraint::Min(cpu_width as u16)].as_ref())
                    .split(v_sections[0]);
                Block::default().title("Network").borders(Borders::ALL).render(&mut f, v_sections[2]);
                let net =
                    Layout::default()
                        .margin(1)
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
                    .split(v_sections[2]);
                Block::default().title("Disk").borders(Borders::ALL).render(&mut f, v_sections[3]);
                let disk_layout = Layout::default().margin(0).direction(Direction::Horizontal)
                .constraints([Constraint::Length(20), Constraint::Min(10)].as_ref()).split(v_sections[3]);




                render_cpu_histogram(&app, cpu_mem[0], &mut f);

                render_memory_histogram(&app, cpu_mem[1], &mut f);

                render_process_table(&app, width, v_sections[4], *pst,&mut f);
                if v_sections[4].height > 4{ // account for table border & margins.
                    process_table_height = v_sections[4].height - 5;
                }
                render_cpu_bars(&app, h_sections[1], cpu_width as u16, &mut f);

                render_overview(&app, h_sections[0], hostname.as_str(), &mut f);

                render_net(&app, net, &mut f);
                render_disk(&app, disk_layout, &mut f);

            }).expect("Could not draw frame.");

            match self.events.next().expect("No new event.") {
                Event::Input(input) => {
                    if input == Key::Char('q') {
                        break;
                    }
                    else if input == Key::Up{
                        self.app.highlight_up();
                        if self.process_table_row_start > 0 && self.app.highlighted_row < self.process_table_row_start{
                            self.process_table_row_start -= 1;
                        }
                    }
                    else if input == Key::Down{
                        self.app.highlight_down();
                        if self.process_table_row_start < self.app.process_map.len() &&
                            self.app.highlighted_row > (self.process_table_row_start + process_table_height as usize){
                            self.process_table_row_start += 1;
                        }
                    }
                    else if input == Key::Char('.') || input == Key::Char('>'){
                        if self.app.psortby == ProcessTableSortBy::Cmd{
                            self.app.psortby = ProcessTableSortBy::Pid;
                        }
                        else{
                            self.app.psortby = num::FromPrimitive::from_u32(self.app.psortby as u32 + 1).unwrap();
                        }
                        self.app.sort_process_table();
                    }
                    else if input == Key::Char(',')  || input == Key::Char('<'){
                        if self.app.psortby == ProcessTableSortBy::Pid{
                            self.app.psortby = ProcessTableSortBy::Cmd;
                        }
                        else{
                            self.app.psortby = num::FromPrimitive::from_u32(self.app.psortby as u32 - 1).unwrap();
                        }
                        self.app.sort_process_table();
                    }
                    else if input == Key::Char('/'){
                        match self.app.psortorder{
                            ProcessTableSortOrder::Ascending => self.app.psortorder = ProcessTableSortOrder::Descending,
                            ProcessTableSortOrder::Descending => self.app.psortorder = ProcessTableSortOrder::Ascending
                        }
                        self.app.sort_process_table();
                    }
                    
                },
                Event::Tick => {
                    self.app.update(width);
                }
            }
        }
    }
}