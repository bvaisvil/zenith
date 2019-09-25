//#[allow(dead_code)]

extern crate sysinfo;
extern crate hostname;
#[macro_use] extern crate byte_unit;
#[macro_use] extern crate maplit;

use std::io;
use std::error::{Error};
use termion::event::Key;
use termion::input::MouseTerminal;
use termion::raw::IntoRawMode;
use termion::screen::AlternateScreen;
use tui::backend::TermionBackend;
use tui::layout::{Constraint, Direction, Layout};
use tui::style::{Color, Modifier, Style};
use tui::widgets::{BarChart, Block, Borders, Widget, Sparkline, Paragraph, Text, Table, Row};
use tui::Terminal;
use sysinfo::{NetworkExt, System, SystemExt, ProcessorExt, DiskExt, Pid, ProcessExt, Process, ProcessStatus};
use byte_unit::{Byte, ByteUnit};
use users::{User, UsersCache, Users};
use hostname::get_hostname;

use std::sync::mpsc;
use std::thread;
use std::task::{Poll};
use std::time::Duration;
use std::collections::{HashMap};

use std::panic::{PanicInfo};
use std::panic;

use termion::input::TermRead;


use rand::distributions::{Distribution, Uniform};
use rand::rngs::ThreadRng;

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

struct ZProcess{
    pid: i32,
    uid: u32,
    user_name: String,
    memory: u64,
    cpu_usage: f32,
    command: Vec<String>,
    exe: String,
    status: ProcessStatus,
    name: String
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
    processes: Vec<ZProcess>,
    user_cache: UsersCache
}

impl<'a> CPUTimeApp<'a>{
    fn new () -> CPUTimeApp<'a>{
        CPUTimeApp{
            cpu_usage_histogram: vec![],
            mem_usage_histogram: vec![],
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
            processes: vec![],
            user_cache: UsersCache::new()
        }
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
        self.processes.clear();
        for (pid, process) in self.system.get_process_list(){
            self.processes.push( ZProcess{
                uid: process.uid,
                user_name: self.user_cache.get_user_by_uid(process.uid).unwrap().name().to_string_lossy().to_string(),
                pid: pid.clone(),
                memory: process.memory(),
                cpu_usage: process.cpu_usage(),
                command: process.cmd().to_vec(),
                status: process.status(),
                exe: format!("{}", process.exe().display()),
                name: process.name().to_string()
            });
        }
        self.processes.sort_by(|a, b| a.cpu_usage.partial_cmp(&b.cpu_usage).unwrap());
        self.processes.reverse();
    }
}


fn mem_title(app: &CPUTimeApp) -> String {
    let mut mem: u64 = 0;
    if app.mem_utilization > 0 && app.mem_total > 0{
        mem = ((app.mem_utilization as f32 / app.mem_total as f32) * 100.0) as u64;
    }
    let mut swp: u64 = 0;
    if app.swap_utilization > 0 && app.swap_total > 0{
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
    format!("CPU [{: >3}%] UP [{:.2}] DN [{:.2}]",
            app.cpu_utilization,
            Byte::from_unit(app.net_out as f64, ByteUnit::B).unwrap().get_appropriate_unit(false),
            Byte::from_unit(app.net_in as f64, ByteUnit::B).unwrap().get_appropriate_unit(false)
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

fn main() -> Result<(), Box<dyn Error>> {
    panic::set_hook(Box::new(|info| { panic_hook(info);}));
    // Terminal initialization
    let stdout = io::stdout().into_raw_mode().expect("Could not bind to STDOUT in raw mode.");
    let stdout = MouseTerminal::from(stdout);
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend).expect("Could not create new terminal.");
    terminal.hide_cursor().expect("Hiding cursor failed.");

    // Setup event handlers
    let events = Events::new();

    let mut app = CPUTimeApp::new();
    let hostname = get_hostname().unwrap();

    loop {

        let mut width: u16 = 0;
        terminal.draw(|mut f| {
            // primary layout division.
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(2)
                .constraints([
                    Constraint::Length(8),
                    Constraint::Percentage(20),
                    Constraint::Percentage(20),
                    Constraint::Min(10)
                ].as_ref())
                .split(f.size());
            width = f.size().width;

            // CPU sparkline
            let title =  cpu_title(&app);
            Sparkline::default()
                .block(
                    Block::default().title(title.as_str()).borders(Borders::ALL))
                .data(&app.cpu_usage_histogram)
                .style(Style::default().fg(Color::Blue))
                .max(100)
                .render(&mut f, chunks[1]);

            // memory sparkline
            let title2 =  mem_title(&app);
            Sparkline::default()
                .block(
                    Block::default().title(title2.as_str()).borders(Borders::ALL))
                .data(&app.mem_usage_histogram)
                .style(Style::default().fg(Color::Cyan))
                .max(100)
                .render(&mut f, chunks[2]);


            // process table
            let rows = app.processes.iter().map(|p|{
                vec![
                    format!("{: >5}", p.pid),
                    format!("{: >10}", p.user_name),
                    format!("{:>.1}", p.cpu_usage),
                    format!("{:>.1}", (p.memory as f64 / app.mem_utilization as f64) * 100.0),
                    format!("{: >8}", Byte::from_unit(p.memory as f64, ByteUnit::KB)
                        .unwrap().get_appropriate_unit(false)).replace(" ", "").replace("B", ""),
                    format!("{:1}", p.status.to_single_char()),
                    format!("{}", p.command.join(" ")) + &[p.exe.as_str(), p.name.as_str()].join(" ")
                ]
            });
            let mut rows = rows.enumerate().map(|(i, r)|{
                if i == 0{
                    Row::StyledData(r.into_iter(), Style::default().fg(Color::Magenta))
                }
                else{
                    Row::Data(r.into_iter())
                }

            });
            let mut cmd_width = width as i16 - 47;
            if cmd_width < 0{
                cmd_width = 0;
            }
            let cmd_width = cmd_width as u16;
            let mut cmd_header = String::from("CMD");
            for i in 3..cmd_width{
                cmd_header += " ";
            }
            let header = [ "PID   ",
                                    "USER       ",
                                    "CPU%  ",
                                    "MEM%  ",
                                    "MEM     ",
                                    "S ",
                                    cmd_header.as_str()];
            Table::new(header.into_iter(), rows)
                .block(Block::default().borders(Borders::ALL)
                                       .title(format!("{} Running Tasks",
                                                      app.processes.len()).as_str()))
                .widths(&[6, 11, 6, 6, 8, 2, cmd_width ])
                .column_spacing(0)
                .header_style(Style::default().bg(Color::DarkGray))
                .render(&mut f, chunks[3]);

            {
                let cpus = app.cpus.as_slice();
                let mut xz :Vec<(&str, u64)> = vec![];
                for (p, u) in cpus.iter(){
                    xz.push((p.as_str(), u.clone()));
                }
                let overview_width: u16 = (3 + 2) * 4;
                 let mut cpu_width: i16 = width as i16 - overview_width as i16;
                 if cpu_width < 1 {
                     cpu_width = 1;
                 }
                // secondary UI division
                let chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Length(overview_width), Constraint::Min(cpu_width as u16)].as_ref())
                    .split(chunks[0]);

                // bit messy way to calc cpu bar width..
                let mut np = app.cpus.len() as u16;
                if np == 0{
                    np = 1;
                }
                let mut cpu_bw = (((cpu_width as f32) - (np as f32 * 2.0)) / np as f32) as i16;
                if cpu_bw < 1{
                    cpu_bw = 1;
                }
                let cpu_bw = cpu_bw as u16;

                //println!("{}{:?}\r", termion::screen::ToMainScreen, &app.overview);
                // Bar chart for current CPU usage.
                BarChart::default()
                    .block(Block::default().title(format!("CPU(S) [{}]", np).as_str()).borders(Borders::ALL))
                    .data(xz.as_slice())
                    .bar_width(cpu_bw)
                    .bar_gap(1)
                    .max(100)
                    .style(Style::default().fg(Color::Green))
                    .value_style(Style::default().bg(Color::Green).modifier(Modifier::BOLD))
                    .render(&mut f, chunks[1]);

                // Bar Chart for current overview
                BarChart::default()
                    .block(Block::default().title(hostname.as_str()).borders(Borders::ALL))
                    .data(&app.overview)
                    .style(Style::default().fg(Color::Red))
                    .bar_width(3)
                    .bar_gap(1)
                    .max(100)
                    .value_style(Style::default().bg(Color::Red))
                    .label_style(Style::default().fg(Color::Cyan).modifier(Modifier::ITALIC))
                    .render(&mut f, chunks[0]);
            }
        }).expect("Could not draw frame.");

        match events.next().expect("No new event.") {
            Event::Input(input) => {
                if input == Key::Char('q') {
                    break;
                }
            }
            Event::Tick => {
                app.update(width);
            }
        }
    }

    Ok(())
}