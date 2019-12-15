/**
 * Copyright 2019 Benjamin Vaisvil (ben@neuon.com)
 */
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::{Color, Modifier, Style};
use sysinfo::{DiskExt};
use tui::widgets::{BarChart, Block, Borders, Widget, Sparkline, Text, Table, Row, List, Paragraph};
use byte_unit::{Byte, ByteUnit};
use termion::event::Key;
use termion::input::MouseTerminal;
use termion::raw::{IntoRawMode, RawTerminal};
use termion::screen::AlternateScreen;
use tui::backend::TermionBackend;
use tui::Terminal;
use std::io::{Stdout};
use tui::Frame;
use std::io;
use crate::util::*;
use crate::metrics::*;
use crate::zprocess::*;
use std::borrow::Cow;
use std::time::{SystemTime, Duration};

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
    let h = match app.histogram_map.get("cpu_usage_histogram"){
        Some(h) => &h.data,
        None => return String::from("")
    };
    let mean: f32 = match h.len(){
        0 => 0.0,
        _ => h.iter().sum::<u64>() as f32 / h.len() as f32,
    };
    format!("CPU [{: >3}%] MEAN [{: >3.1}%] TOP [{} - {} - {}]",
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

    if area.height < 5{
        return; // not enough space to draw anything
    }
    let display_height = area.height as usize - 4; // 4 for the margins and table header

    let end = process_table_start + display_height;

    let rows: Vec<Vec<String>> = app.processes.iter().map(|pid| {
        let p = app.process_map.get(pid).unwrap();
        vec![
            format!("{: >5}", p.pid),
            format!("{: <10}", p.user_name),
            format!("{: <3}", p.priority),
            format!("{:>5.1}", p.cpu_usage),
            format!("{:>5.1}", (p.memory as f64 / app.mem_total as f64) * 100.0),
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
    //figure column widths
    let mut widths: Vec<u16> = header.iter().map(|item| item.len() as u16).collect();
    let s: u16 = widths.iter().sum();
    let mut cmd_width = width as i16 - s as i16 - 3;
    if cmd_width < 0 {
        cmd_width = 0;
    }
    let cmd_width = cmd_width as u16;
    let mut cmd_header = String::from("CMD");
    for _i in 3..cmd_width {
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
               .title(format!("Tasks [{}] Threads [{}]  Navigate [↑/↓] Sort Col [,/.] Asc/Dec [/]", app.processes.len(), app.threads_total).as_str()))
        .widths(widths.as_slice())
        .column_spacing(0)
        .header_style(Style::default().bg(Color::DarkGray)).render(f, area);
}

fn render_cpu_histogram(app: &CPUTimeApp, area: Rect, 
f: &mut Frame<TermionBackend<AlternateScreen<MouseTerminal<RawTerminal<Stdout>>>>>){
    let title = cpu_title(&app);
    let h = match app.histogram_map.get("cpu_usage_histogram"){
        Some(h) => &h.data,
        None => return
    };
    let start_at = if h.len() > area.width as usize
    {
        h.len() - area.width as usize
    }
    else{
        0
    };
    Sparkline::default()
        .block(
            Block::default().title(title.as_str()))
        .data(&h[start_at..])
        .style(Style::default().fg(Color::Blue))
        .max(100)
        .render(f, area);
}

fn render_memory_histogram(app: &CPUTimeApp, area: Rect, 
    f: &mut Frame<TermionBackend<AlternateScreen<MouseTerminal<RawTerminal<Stdout>>>>>) {
    let h = match app.histogram_map.get("mem_utilization"){
        Some(h) => &h.data,
        None => return
    };
    let title2 = mem_title(&app);
    let start_at = if h.len() > area.width as usize
    {
        h.len() - area.width as usize
    }
    else{
        0
    };
    Sparkline::default()
        .block(
            Block::default().title(title2.as_str()))
        .data(&h[start_at..])
        .style(Style::default().fg(Color::Cyan))
        .max(100)
        .render(f, area);
}

fn render_cpu_bars(app: &CPUTimeApp, area: Rect, width: u16, 
    f: &mut Frame<TermionBackend<AlternateScreen<MouseTerminal<RawTerminal<Stdout>>>>>){

    let mut cpus = app.cpus.to_owned();
    let mut bars: Vec<(&str, u64)> = vec![];
    let mut bar_gap: u16 = 1;

    let mut np = cpus.len() as u16;
    if np == 0 {
        np = 1;
    }
    
    let mut constraints: Vec<Constraint> = vec![];
    let mut half: usize = np as usize;
    if np > width - 2{
        constraints.push(Constraint::Percentage(50));
        constraints.push(Constraint::Percentage(50));
        half = np as usize / 2;
    }
    else{
        constraints.push(Constraint::Percentage(100));
    }
    //compute bar width and gutter/gap

    if width > 2 && (half * 2) >= (width - 2) as usize{
        bar_gap = 0;
    }
    let width = width - 2;
    let mut cpu_bw = ((width as f32 - (half as u16 * bar_gap) as f32) / half as f32) as i16;
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

    Block::default().title(format!("CPU(S) {}@{} MHz", np, app.frequency).as_str())
                    .borders(Borders::ALL)
                    .render(f, area);
    let cpu_bar_layout = Layout::default().margin(1)
                                          .direction(Direction::Vertical)
                                          .constraints(constraints.as_ref())
                                          .split(area);
    
    if np > width{
        BarChart::default()
        .data(&bars[0..half])
        .bar_width(cpu_bw)
        .bar_gap(bar_gap)
        .max(100)
        .style(Style::default().fg(Color::Green))
        .value_style(Style::default().bg(Color::Green).modifier(Modifier::BOLD))
        .render(f, cpu_bar_layout[0]);
        BarChart::default()
        .data(&bars[half..])
        .bar_width(cpu_bw)
        .bar_gap(bar_gap)
        .max(100)
        .style(Style::default().fg(Color::Green))
        .value_style(Style::default().bg(Color::Green).modifier(Modifier::BOLD))
        .render(f, cpu_bar_layout[1]);
    }
    else{
        BarChart::default()
        .data(bars.as_slice())
        .bar_width(cpu_bw)
        .bar_gap(bar_gap)
        .max(100)
        .style(Style::default().fg(Color::Green))
        .value_style(Style::default().bg(Color::Green).modifier(Modifier::BOLD))
        .render(f, cpu_bar_layout[0]);
    }
    
}


fn render_net(app: &CPUTimeApp, area: Vec<Rect>,
              f: &mut Frame<TermionBackend<AlternateScreen<MouseTerminal<RawTerminal<Stdout>>>>>){
    let net = Layout::default()
                .margin(1)
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
                .split(area[1]);
    
    let net_up = Byte::from_unit(app.net_out as f64, ByteUnit::B).unwrap().get_appropriate_unit(false);
    let h_out = match app.histogram_map.get("net_out"){
        Some(h) => &h.data,
        None => return
    };
    let start_at = if h_out.len() > net[0].width as usize
    {
        h_out.len() - net[0].width as usize
    }
    else{
        0
    };

    let up_max: u64 = match h_out[start_at..].iter().max(){
        Some(x) => x.clone(),
        None => 1
    };
    let up_max_bytes =  Byte::from_unit(up_max as f64, ByteUnit::B).unwrap().get_appropriate_unit(false);

    Sparkline::default()
        .block(Block::default().title(format!("↑ [{:^10}] Max [{:^10}]", net_up.to_string(),
         up_max_bytes.to_string()).as_str()))
        .data(&h_out[start_at..])
        .style(Style::default().fg(Color::LightYellow))
        .max(up_max)
        .render(f, net[0]);

  
    let net_down = Byte::from_unit(app.net_in as f64, ByteUnit::B).unwrap().get_appropriate_unit(false);
    let h_in = match app.histogram_map.get("net_in"){
        Some(h) => &h.data,
        None => return
    };
    let start_at = if h_in.len() > net[1].width as usize
    {
        h_in.len() - net[1].width as usize
    }
    else{
        0
    };
    let down_max: u64 = match h_in[start_at..].iter().max(){
        Some(x) => x.clone(),
        None => 1
    };
    let down_max_bytes =  Byte::from_unit(down_max as f64, ByteUnit::B).unwrap().get_appropriate_unit(false);
    Sparkline::default()
        .block(Block::default().title(format!("↓ [{:^10}] Max [{:^10}]", net_down.to_string(), down_max_bytes.to_string()).as_str()))
        .data(&h_in[start_at..])
        .style(Style::default().fg(Color::LightMagenta))
        .max(down_max)
        .render(f, net[1]);

    let ips = app.network_interfaces.iter()
    .map(|n| Text::Styled(Cow::Owned(format!("{:<8.8} : {}", n.name, n.ip)), Style::default().fg(Color::Green) ));
    List::new(ips).block(Block::default().title("Network").borders(Borders::ALL)).render(f, area[0]);
}

fn render_disk(app: &CPUTimeApp, disk_layout: Vec<Rect>,
              f: &mut Frame<TermionBackend<AlternateScreen<MouseTerminal<RawTerminal<Stdout>>>>>){
    let area = Layout::default().margin(1).direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref()).split(disk_layout[1]);
    

    let read_up = Byte::from_unit(app.disk_read as f64, ByteUnit::B).unwrap().get_appropriate_unit(false);
    let h_read = match app.histogram_map.get("disk_read"){
        Some(h) => &h.data,
        None => return
    };
    let start_at = if h_read.len() > area[0].width as usize
    {
        h_read.len() - area[0].width as usize
    }
    else{
        0
    };
    let read_max: u64 = match h_read[start_at..].iter().max(){
        Some(x) => x.clone(),
        None => 1
    };
    let read_max_bytes =  Byte::from_unit(read_max as f64, ByteUnit::B).unwrap().get_appropriate_unit(false);
    Sparkline::default()
        .block(Block::default().title(format!("R [{:^10}] Max [{:^10}]", read_up.to_string(), read_max_bytes.to_string()).as_str()))
        .data(&h_read[start_at..])
        .style(Style::default().fg(Color::LightYellow))
        .max(read_max)
        .render(f, area[0]);


    
    let write_down = Byte::from_unit(app.disk_write as f64, ByteUnit::B).unwrap().get_appropriate_unit(false);
    let h_write = match app.histogram_map.get("disk_write"){
        Some(h) => &h.data,
        None => return
    };
    let start_at = if h_write.len() > area[1].width as usize
    {
        h_write.len() - area[1].width as usize
    }
    else{
        0
    };
    let write_max: u64 = match h_write[start_at..].iter().max(){
        Some(x) => x.clone(),
        None => 1
    };
    let write_max_bytes =  Byte::from_unit(write_max as f64, ByteUnit::B).unwrap().get_appropriate_unit(false);
    Sparkline::default()
        .block(Block::default().title(format!("W [{:^10}] Max [{:^10}]", write_down.to_string(), write_max_bytes.to_string()).as_str()))
        .data(&h_write[start_at..])
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


pub struct TerminalRenderer{
    terminal: Terminal<TermionBackend<AlternateScreen<MouseTerminal<RawTerminal<Stdout>>>>>,
    app: CPUTimeApp,
    events: Events,
    process_table_row_start: usize,
    cpu_height: u16,
    net_height: u16,
    disk_height: u16,
    process_height: u16
}

impl<'a> TerminalRenderer {
    pub fn new(tick_rate: u64, cpu_height: u16, net_height: u16, disk_height: u16, process_height: u16) -> TerminalRenderer {
        let stdout = io::stdout().into_raw_mode().expect("Could not bind to STDOUT in raw mode.");
        let stdout = MouseTerminal::from(stdout);
        let stdout = AlternateScreen::from(stdout);
        let backend = TermionBackend::new(stdout);
        TerminalRenderer {
            terminal: Terminal::new(backend).unwrap(),
            app: CPUTimeApp::new(Duration::from_millis(tick_rate)),
            events: Events::new(tick_rate),
            process_table_row_start: 0,
            cpu_height,
            net_height,
            disk_height,
            process_height
        }
    }

    pub async fn start(&mut self) {
        loop {
            let app = &self.app;
            let hostname = self.app.hostname.as_str();
            let os = self.app.osname.as_str();
            let release = self.app.release.as_str();
            let arch = self.app.arch.as_str();
            let version = self.app.version.as_str();
            let pst = &self.process_table_row_start;
            let cpu_height = &self.cpu_height;
            let net_height = &self.net_height;
            let disk_height = &self.disk_height;
            let process_height = &self.process_height;
            let mut width: u16 = 0;
            let mut process_table_height: u16 = 0;
            let pname = self.app.processor_name.as_str();
            self.terminal.draw( |mut f| {
                width = f.size().width;
                // primary layout division.
                let mut constraints = vec![
                    Constraint::Length(1),
                    Constraint::Length(*cpu_height),
                    Constraint::Length(*net_height),
                    Constraint::Length(*disk_height)];
                if *process_height > 0{
                    constraints.push(Constraint::Min(*process_height));
                }
                
                let v_sections = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(0)
                    .constraints(constraints.as_ref())
                    .split(f.size());
                let default_style = Style::default().bg(Color::DarkGray).fg(Color::White);
                let line = vec![
                    Text::styled(format!(" {:}", hostname), default_style.modifier(Modifier::BOLD)),
                    //Text::styled(format!(" [{:}]", pname), default_style),
                    Text::styled(format!(" [{:} {:}]", os, release), default_style),
                    Text::styled(" (q)uit", default_style),
                    Text::styled(format!("{: >width$}", "", width=width as usize), default_style),
                ];
                Paragraph::new(line.iter()).render(&mut f, v_sections[0]);
                Block::default()
                      .title("")
                      .title_style(Style::default().modifier(Modifier::BOLD).fg(Color::Red))
                      .borders(Borders::ALL).render(&mut f, v_sections[1]);
                let cpu_layout = Layout::default().margin(0).direction(Direction::Horizontal)
                                        .constraints([Constraint::Length(30), Constraint::Min(10)].as_ref())
                                        .split(v_sections[1]);

                let cpu_mem = Layout::default().margin(1).direction(Direction::Vertical)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
                    .split(cpu_layout[1]);

                Block::default().title("Network").borders(Borders::ALL)
                .render(&mut f, v_sections[2]);
                let network_layout = Layout::default().margin(0).direction(Direction::Horizontal)
                .constraints([Constraint::Length(30), Constraint::Min(10)].as_ref()).split(v_sections[2]);
                
                Block::default().title("Disk").borders(Borders::ALL).render(&mut f, v_sections[3]);
                let disk_layout = Layout::default().margin(0).direction(Direction::Horizontal)
                .constraints([Constraint::Length(30), Constraint::Min(10)].as_ref()).split(v_sections[3]);

                render_cpu_histogram(&app, cpu_mem[0], &mut f);

                render_memory_histogram(&app, cpu_mem[1], &mut f);

                if *process_height > 0{
                    render_process_table(&app, width, v_sections[4], *pst,&mut f);
                    if v_sections[4].height > 4{ // account for table border & margins.
                        process_table_height = v_sections[4].height - 5;
                    }
                }

                render_cpu_bars(&app, cpu_layout[0], 30, &mut f);

                render_net(&app, network_layout, &mut f);
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
                    self.app.update(width).await;
                }
            }
        }
    }
}