//#[allow(dead_code)]

extern crate sysinfo;
extern crate hostname;
#[macro_use] extern crate byte_unit;
#[macro_use] extern crate maplit;

use std::io;
use std::error::{Error};
use termion::event::Key;
use termion::input::MouseTerminal;
use termion::raw::{IntoRawMode, RawTerminal};
use termion::screen::AlternateScreen;
use std::fmt::Display;
use tui::backend::TermionBackend;
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::{Color, Modifier, Style};
use tui::widgets::{BarChart, Block, Borders, Widget, Sparkline, Paragraph, Text, Table, Row};
use tui::Terminal;
use tui::Frame;
use sysinfo::{NetworkExt, System, SystemExt, ProcessorExt, DiskExt, Pid, ProcessExt, Process, ProcessStatus};
use byte_unit::{Byte, ByteUnit};
use users::{User, UsersCache, Users, Groups};
use hostname::get_hostname;
use sys_info;

use std::sync::{mpsc, Arc};
use std::thread;
use std::task::{Poll};
use std::time::Duration;
use std::collections::{HashMap, HashSet};

use std::panic::{PanicInfo};
use std::panic;

use termion::input::TermRead;


use rand::distributions::{Distribution, Uniform};
use rand::rngs::ThreadRng;
use std::io::{Write, Stdout};

pub struct TabsState<'a> {
    pub titles: Vec<&'a str>,
    pub index: usize,
}

impl<'a> TabsState<'a> {
    pub fn new(titles: Vec<&'a str>) -> TabsState {
        TabsState { titles, index: 0 }
    }
    pub fn next(&mut self) {
        self.index = (self.index + 1) % self.titles.len();
    }

    pub fn previous(&mut self) {
        if self.index > 0 {
            self.index -= 1;
        } else {
            self.index = self.titles.len() - 1;
        }
    }
}


pub enum Event<I> {
    Input(I),
    Tick,
}

/// A small event handler that wrap termion input and tick events. Each event
/// type is handled in its own thread and returned to a common `Receiver`
pub struct Events {
    rx: mpsc::Receiver<Event<Key>>,
    input_handle: thread::JoinHandle<()>,
    tick_handle: thread::JoinHandle<()>,
}

#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub exit_key: Key,
    pub tick_rate: Duration,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            exit_key: Key::Char('q'),
            tick_rate: Duration::from_millis(1000),
        }
    }
}

impl Events {
    pub fn new() -> Events {
        Events::with_config(Config::default())
    }

    pub fn with_config(config: Config) -> Events {
        let (tx, rx) = mpsc::channel();
        let input_handle = {
            let tx = tx.clone();
            thread::spawn(move || {
                let stdin = io::stdin();
                for evt in stdin.keys() {
                    match evt {
                        Ok(key) => {
                            if let Err(_) = tx.send(Event::Input(key)) {
                                return;
                            }
                            if key == config.exit_key {
                                return;
                            }
                        }
                        Err(_) => {}
                    }
                }
            })
        };
        let tick_handle = {
            let tx = tx.clone();
            thread::spawn(move || {
                let tx = tx.clone();
                loop {
                    tx.send(Event::Tick).unwrap();
                    thread::sleep(config.tick_rate);
                }
            })
        };
        Events {
            rx,
            input_handle,
            tick_handle,
        }
    }

    pub fn next(&self) -> Result<Event<Key>, mpsc::RecvError> {
        self.rx.recv()
    }
}

#[derive(Clone)]
struct ZProcess{
    pid: i32,
    uid: u32,
    user_name: String,
    memory: u64,
    cpu_usage: f32,
    cum_cpu_usage: f64,
    command: Vec<String>,
    exe: String,
    status: ProcessStatus,
    name: String,
    priority: i32,
    virtual_memory: u64
}

struct CPUTimeApp<'a> {
    cpu_usage_histogram: Vec<u64>,
    cpu_utilization: u64,
    mem_utilization: u64,
    mem_total: u64,
    mem_usage_histogram: Vec<u64>,
    swap_utilization: u64,
    swap_total: u64,
    disk_total: u64,
    disk_available: u64,
    cpus: Vec<(String, u64)>,
    system: System,
    overview: Vec<(&'a str, u64)>,
    net_in: u64,
    net_out: u64,
    processes: Vec<i32>,
    process_map: HashMap<i32, ZProcess>,
    user_cache: UsersCache,
    cum_cpu_process: Option<i32>,
    frequency: u64,
    highlighted_row: usize,
}

impl<'a> CPUTimeApp<'a>{
    fn new () -> CPUTimeApp<'a>{
        CPUTimeApp{
            cpu_usage_histogram: Vec::with_capacity(60),
            mem_usage_histogram: Vec::with_capacity(60),
            cpus: vec![],
            system: System::new(),
            cpu_utilization: 0,
            mem_utilization: 0,
            mem_total: 0,
            swap_total: 0,
            swap_utilization: 0,
            disk_available: 0,
            disk_total: 0,
            overview: vec![
                ("CPU", 0),
                ("MEM", 0),
                ("SWP", 0),
                ("DSK", 0)
            ],
            net_in: 0,
            net_out: 0,
            processes: Vec::with_capacity(400),
            process_map: HashMap::with_capacity(400),
            user_cache: UsersCache::new(),
            cum_cpu_process: Option::from(0),
            frequency: 0,
            highlighted_row: 0
        }
    }

    fn highlight_up(&mut self){
        if self.highlighted_row != 0{
            self.highlighted_row -= 1;
        }
    }

    fn highlight_down(&mut self){
        if self.highlighted_row < self.process_map.len(){
            self.highlighted_row += 1;
        }
    }

    fn update_process_list(&mut self){
        self.processes.clear();
        let process_list = self.system.get_process_list();
        let mut current_pids: HashSet<i32> = HashSet::with_capacity(process_list.len());
        let mut top_pid = 0;
        let mut top_cum_cpu_usage: f64 = 0.0;

        for (pid, process) in process_list{
            if self.process_map.contains_key(pid){
                let zp = self.process_map.get_mut(pid).unwrap();
                zp.memory = process.memory();
                zp.cpu_usage = process.cpu_usage();
                zp.cum_cpu_usage += zp.cpu_usage as f64;
                zp.status = process.status();
                zp.priority = process.priority;
                zp.virtual_memory = process.virtual_memory;
                if zp.cum_cpu_usage > top_cum_cpu_usage{
                    top_pid = zp.pid;
                    top_cum_cpu_usage = zp.cum_cpu_usage;
                }
            }
            else{
                let user_name = match self.user_cache.get_user_by_uid(process.uid){
                    Some(user) => user.name().to_string_lossy().to_string(),
                    None => String::from("")
                };
                let zprocess = ZProcess{
                    uid: process.uid,
                    user_name: user_name,
                    pid: pid.clone(),
                    memory: process.memory(),
                    cpu_usage: process.cpu_usage(),
                    command: process.cmd().to_vec(),
                    status: process.status(),
                    exe: format!("{}", process.exe().display()),
                    name: process.name().to_string(),
                    cum_cpu_usage: process.cpu_usage() as f64,
                    priority: process.priority,
                    virtual_memory: process.virtual_memory
                };
                if zprocess.cum_cpu_usage > top_cum_cpu_usage{
                    top_pid = zprocess.pid;
                    top_cum_cpu_usage = zprocess.cum_cpu_usage;
                }
                self.process_map.insert(zprocess.pid, zprocess);
            }
            self.processes.push(pid.clone());
            current_pids.insert(pid.clone());
        }

        // remove pids that are gone
        self.process_map.retain(|&k, _| current_pids.contains(&k));

        let pm = &self.process_map;
        self.processes.sort_by(|a, b| {
            let pa =pm.get(a).unwrap();
            let pb = pm.get(b).unwrap();
            pa.cpu_usage.partial_cmp(&pb.cpu_usage).unwrap()
        });
        self.processes.reverse();
        self.cum_cpu_process = Option::from(top_pid);
    }

    fn update_frequency(&mut self){
        self.frequency = sys_info::cpu_speed().unwrap_or(0);
    }

    fn update(&mut self, width: u16) {
        self.system.refresh_all();
        let procs = self.system.get_processor_list();
        let mut num_procs = 1;
        let mut usage: f32 = 0.0;
        self.cpus.clear();
        for p in procs.iter().skip(1){
            let u = p.get_cpu_usage();
            self.cpus.push((format!("{}", num_procs), (u * 100.0) as u64));
            usage += u;
            num_procs += 1;
        }
        let usage = usage / num_procs as f32;
        self.cpu_utilization = (usage * 100.0) as u64;
        self.overview[0] = ("CPU", self.cpu_utilization);
        self.cpu_usage_histogram.push((usage * 100.0) as u64);
        if self.cpu_usage_histogram.len() > width as usize{
            self.cpu_usage_histogram.remove(0);
        }

        self.mem_utilization = self.system.get_used_memory();
        self.mem_total = self.system.get_total_memory();

        let mut mem: u64 = 0;
        if self.mem_total > 0{
            mem = ((self.mem_utilization as f64/ self.mem_total as f64) * 100.0) as u64;
        }


        self.overview[1] = ("MEM", mem);
        self.mem_usage_histogram.push(mem);
        if self.mem_usage_histogram.len() > width as usize{
            self.mem_usage_histogram.remove(0);
        }

        self.swap_utilization = self.system.get_used_swap();
        self.swap_total = self.system.get_total_swap();


        let mut swp: u64 = 0;
        if self.swap_total > 0 && self.swap_utilization > 0{
            swp = ((self.swap_utilization as f64/ self.swap_total as f64) * 100.0) as u64;
        }
        self.overview[2] = ("SWP", swp);

        self.disk_available = 0;
        self.disk_total = 0;

        for d in self.system.get_disks().iter(){
            self.disk_available += d.get_available_space();
            self.disk_total += d.get_total_space();
        }

        let du = self.disk_total - self.disk_available;
        self.overview[3] = ("DSK", ((du as f32 / self.disk_total as f32) * 100.0) as u64);


        let net = self.system.get_network();

        self.net_in = net.get_income();
        self.net_out = net.get_outcome();
        self.update_process_list();
        self.update_frequency();
    }
}


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
            Byte::from_unit(app.mem_total as f64, ByteUnit::KB).unwrap().get_appropriate_unit(false).to_string().replace(" ", ""),
            mem,
            Byte::from_unit(app.swap_total as f64, ByteUnit::KB).unwrap().get_appropriate_unit(false).to_string().replace(" ", ""),
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
    format!("CPU [{: >3}%] UP [{:.2}] DN [{:.2}] TOP [{} - {} - {}]",
            app.cpu_utilization,
            Byte::from_unit(app.net_out as f64, ByteUnit::B).unwrap().get_appropriate_unit(false),
            Byte::from_unit(app.net_in as f64, ByteUnit::B).unwrap().get_appropriate_unit(false),
            top_pid,
            top_process_name,
            top_process_amt
    )
}
pub trait ProcessStatusExt{
    fn to_single_char(&self) -> &str;
}

impl ProcessStatusExt for ProcessStatus{
    #[cfg(target_os = "macos")]
    fn to_single_char(&self) -> &str{
        match *self{
            ProcessStatus::Idle       => "I",
            ProcessStatus::Run        => "R",
            ProcessStatus::Sleep      => "S",
            ProcessStatus::Stop       => "T",
            ProcessStatus::Zombie     => "Z",
            ProcessStatus::Unknown(_) => "U",
        }
    }

    #[cfg(all(any(unix), not(target_os = "macos")))]
    fn to_single_char(&self) -> &str{
        match *self {
            ProcessStatus::Idle       => "I",
            ProcessStatus::Run        => "R",
            ProcessStatus::Sleep      => "S",
            ProcessStatus::Stop       => "T",
            ProcessStatus::Zombie     => "Z",
            ProcessStatus::Tracing    => "t",
            ProcessStatus::Dead       => "x",
            ProcessStatus::Wakekill   => "K",
            ProcessStatus::Waking     => "W",
            ProcessStatus::Parked     => "P",
            ProcessStatus::Unknown(_) => "U",
        }
    }
}

fn panic_hook(info: &PanicInfo<'_>) {
	let location = info.location().unwrap();  // The current implementation always returns Some
	let msg = match info.payload().downcast_ref::<&'static str>() {
		Some(s) => *s,
		None => match info.payload().downcast_ref::<String>() {
			Some(s) => &s[..],
			None => "Box<Any>",
		}
	};
	println!("{}thread '<unnamed>' panicked at '{}', {}\r", termion::screen::ToMainScreen, msg, location);
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
            format!("{:>.1}", p.cpu_usage),
            format!("{:>.1}", (p.memory as f64 / app.mem_utilization as f64) * 100.0),
            format!("{: >8}", Byte::from_unit(p.memory as f64, ByteUnit::KB)
                .unwrap().get_appropriate_unit(false)).replace(" ", "").replace("B", ""),
            format!("{: >8}", Byte::from_unit(p.virtual_memory as f64, ByteUnit::KB)
                .unwrap().get_appropriate_unit(false)).replace(" ", "").replace("B", ""),
            format!("{:1}", p.status.to_single_char()),
            format!("{} - ", p.name) + &[p.exe.as_str(), p.command.join(" ").as_str()].join(" ")
        ]
    }).collect();
    let mut header = vec![
        "PID   ",
        "USER       ",
        "P   ",
        "CPU%  ",
        "MEM%  ",
        "MEM     ",
        "VIRT    ",
        "S "
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
    header.push(cmd_header.as_str());
    widths.push(cmd_header.len() as u16);
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
            .title(format!("{} Running Tasks", app.processes.len()).as_str()))
        .widths(widths.as_slice())
        .column_spacing(0)
        .header_style(Style::default().bg(Color::DarkGray)).render(f, area);

}

fn render_cpu_histogram(app: &CPUTimeApp, area: Rect, f: &mut Frame<TermionBackend<AlternateScreen<MouseTerminal<RawTerminal<Stdout>>>>>){
    let title = cpu_title(&app);
    Sparkline::default()
        .block(
            Block::default().title(title.as_str()).borders(Borders::ALL))
        .data(&app.cpu_usage_histogram)
        .style(Style::default().fg(Color::Blue))
        .max(100)
        .render(f, area);
}

fn render_memory_histogram(app: &CPUTimeApp, area: Rect, f: &mut Frame<TermionBackend<AlternateScreen<MouseTerminal<RawTerminal<Stdout>>>>>) {
    let title2 = mem_title(&app);
    Sparkline::default()
        .block(
            Block::default().title(title2.as_str()).borders(Borders::ALL))
        .data(&app.mem_usage_histogram)
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
        .block(Block::default().title(hostname).borders(Borders::ALL).border_style(Style::default().fg(Color::Red)))
        .data(&app.overview)
        .style(Style::default().fg(Color::Red))
        .bar_width(3)
        .bar_gap(1)
        .max(100)
        .value_style(Style::default().bg(Color::Red))
        .label_style(Style::default().fg(Color::Cyan).modifier(Modifier::ITALIC))
        .render(f, area);
}


struct TerminalRenderer<'a>{
    terminal: Terminal<TermionBackend<AlternateScreen<MouseTerminal<RawTerminal<Stdout>>>>>,
    app: CPUTimeApp<'a>,
    events: Events,
    hostname: String,
    process_table_row_start: usize
}

impl<'a> TerminalRenderer<'a> {
    fn new() -> TerminalRenderer<'a> {
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

    fn start(&mut self) {
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
                        Constraint::Percentage(20),
                        Constraint::Percentage(20),
                        Constraint::Percentage(20),
                        Constraint::Percentage(40)
                    ].as_ref())
                    .split(f.size());


                let overview_width: u16 = 18;
                let mut cpu_width: i16 = width as i16 - overview_width as i16;
                if cpu_width < 1 {
                    cpu_width = 1;
                }

                // secondary layout
                let h_sections = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Length(overview_width),
                        Constraint::Min(cpu_width as u16)].as_ref())
                    .split(v_sections[0]);



                render_cpu_histogram(&app, v_sections[1], &mut f);

                render_memory_histogram(&app, v_sections[2], &mut f);

                render_process_table(&app, width, v_sections[3], *pst,&mut f);
                if v_sections[3].height > 4{ // account for table border & margins.
                    process_table_height = v_sections[3].height - 5;
                }
                render_cpu_bars(&app, h_sections[1], cpu_width as u16, &mut f);

                render_overview(&app, h_sections[0], hostname.as_str(), &mut f);
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
                },
                Event::Tick => {
                    self.app.update(width);
                }
            }
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {

    // Terminal initialization
    let stdout = io::stdout().into_raw_mode().expect("Could not bind to STDOUT in raw mode.");
    let stdout = MouseTerminal::from(stdout);
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend).expect("Could not create new terminal.");
    //terminal.what_is_this();
    terminal.hide_cursor().expect("Hiding cursor failed.");

    panic::set_hook(Box::new(|info| {
        panic_hook(info);
    }));
    let mut r = TerminalRenderer::new();
    r.start();



    Ok(())
}