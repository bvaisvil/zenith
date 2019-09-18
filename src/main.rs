#[allow(dead_code)]

extern crate sysinfo;

use std::io;
use std::error::{Error};
use termion::event::Key;
use termion::input::MouseTerminal;
use termion::raw::IntoRawMode;
use termion::screen::AlternateScreen;
use tui::backend::TermionBackend;
use tui::layout::{Constraint, Direction, Layout};
use tui::style::{Color, Modifier, Style};
use tui::widgets::{BarChart, Block, Borders, Widget, Sparkline, Paragraph, Text};
use tui::Terminal;
use sysinfo::{NetworkExt, System, SystemExt, ProcessorExt, DiskExt};

use std::sync::mpsc;
use std::thread;
use std::task::{Poll};
use std::thread::{sleep_ms};
use std::time::Duration;

use termion::input::TermRead;


use rand::distributions::{Distribution, Uniform};
use rand::rngs::ThreadRng;

#[derive(Clone)]
pub struct RandomSignal {
    distribution: Uniform<u64>,
    rng: ThreadRng,
}

impl RandomSignal {
    pub fn new(lower: u64, upper: u64) -> RandomSignal {
        RandomSignal {
            distribution: Uniform::new(lower, upper),
            rng: rand::thread_rng(),
        }
    }
}

impl Iterator for RandomSignal {
    type Item = u64;
    fn next(&mut self) -> Option<u64> {
        Some(self.distribution.sample(&mut self.rng))
    }
}

#[derive(Clone)]
pub struct SinSignal {
    x: f64,
    interval: f64,
    period: f64,
    scale: f64,
}

impl SinSignal {
    pub fn new(interval: f64, period: f64, scale: f64) -> SinSignal {
        SinSignal {
            x: 0.0,
            interval,
            period,
            scale,
        }
    }
}

impl Iterator for SinSignal {
    type Item = (f64, f64);
    fn next(&mut self) -> Option<Self::Item> {
        let point = (self.x, (self.x * 1.0 / self.period).sin() * self.scale);
        self.x += self.interval;
        Some(point)
    }
}

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
            tick_rate: Duration::from_millis(500),
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
    net_out: u64
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
                ("SWAP", 0),
                ("DISK", 0)
            ],
            net_in: 0,
            net_out: 0
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

        let mem = ((self.mem_utilization as f32/ self.mem_total as f32) * 100.0) as u64;

        self.overview[1] = ("MEM", mem);
        self.mem_usage_histogram.push(mem);
        if self.mem_usage_histogram.len() > width as usize{
            self.mem_usage_histogram.remove(0);
        }

        self.swap_utilization = self.system.get_used_swap();
        self.swap_total = self.system.get_total_swap();

        self.overview[2] = ("SWAP", ((self.swap_utilization as f32/ self.swap_total as f32) * 100.0) as u64);

        self.disk_available = 0;
        self.disk_total = 0;

        for d in self.system.get_disks().iter(){
            self.disk_available += d.get_available_space();
            self.disk_total += d.get_total_space();
        }

        let du = self.disk_total - self.disk_available;
        self.overview[3] = ("DISK", ((du as f32 / self.disk_total as f32) * 100.0) as u64);


        let net = self.system.get_network();

        self.net_in = net.get_income();
        self.net_out = net.get_outcome();

    }
}

struct App<'a> {
    data: Vec<(&'a str, u64)>,
}

impl<'a> App<'a> {
    fn new() -> App<'a> {
        App {
            data: vec![
                ("CPU", 9),
                ("MEM", 12),
                ("SWAP", 5),
                ("NET DOWN", 8),
                ("NET UP", 2),
            ],
        }
    }

    fn update(&mut self) {
    }
}


fn main() -> Result<(), Box<Error>> {
    // Terminal initialization
    let stdout = io::stdout().into_raw_mode()?;
    let stdout = MouseTerminal::from(stdout);
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    // Setup event handlers
    let events = Events::new();

    let mut cpu_time_app = CPUTimeApp::new();

    loop {
        let mut width: u16 = 0;
        terminal.draw(|mut f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(2)
                .constraints([Constraint::Percentage(10),
                    Constraint::Percentage(10),
                    Constraint::Percentage(25),
                    Constraint::Percentage(55)].as_ref())
                .split(f.size());
            width = f.size().width;
            let title =  format!("CPU [{: >3}%] UP [{:.2}] DN [{:.2}]", cpu_time_app.cpu_utilization,
                                 (cpu_time_app.net_out as f64 / 1024.0),
                                 (cpu_time_app.net_in as f64 / 1024.0));
            Sparkline::default()
                .block(
                    Block::default().title(title.as_str()).borders(Borders::ALL))
                .data(&cpu_time_app.cpu_usage_histogram)
                .style(Style::default().fg(Color::Blue))
                .max(100)
                .render(&mut f, chunks[0]);

            let title2 =  format!("MEM [{: >3}%] SWP [{: >3}%]",
                                 ((cpu_time_app.mem_utilization as f32/ cpu_time_app.mem_total as f32) * 100.0) as u64,
                                 ((cpu_time_app.swap_utilization as f32/ cpu_time_app.swap_total as f32) * 100.0) as u64
            );
            Sparkline::default()
                .block(
                    Block::default().title(title2.as_str()).borders(Borders::ALL))
                .data(&cpu_time_app.mem_usage_histogram)
                .style(Style::default().fg(Color::Cyan))
                .max(100)
                .render(&mut f, chunks[1]);
            {
                let cpus = cpu_time_app.cpus.as_slice();
                let mut xz :Vec<(&str, u64)> = vec![];
                for (p, u) in cpus.iter(){
                    xz.push((p.as_str(), u.clone()));
                }
                let overview_width: u16 = (4 + 2) * 4;
                let overview_perc = ((overview_width as f32) / (width as f32) * 100.0) as u16;
                let cpu_width: u16 = width - overview_width;
                let cpu_percw = 100 - overview_perc;

                let chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(cpu_percw), Constraint::Percentage(overview_perc)].as_ref())
                    .split(chunks[2]);

                // bit messy way to calc cpu bar width..
                let mut np = cpu_time_app.cpus.len() as u16;
                if np == 0{
                    np = 1;
                }
                let mut cpu_bw = ((((cpu_width as f32) - (np as f32 * 2.0)) / np as f32)) as u16;
                if cpu_bw < 1{
                    cpu_bw = 1;
                }

                BarChart::default()
                    .block(Block::default().title(format!("CPUS {}", np).as_str()).borders(Borders::ALL))
                    .data(xz.as_slice())
                    .bar_width(cpu_bw)
                    .bar_gap(1)
                    .max(100)
                    .style(Style::default().fg(Color::Green))
                    .value_style(Style::default().bg(Color::Green).modifier(Modifier::BOLD))
                    .render(&mut f, chunks[0]);
                BarChart::default()
                    .block(Block::default().title("Overview").borders(Borders::ALL))
                    .data(&cpu_time_app.overview)
                    .style(Style::default().fg(Color::Red))
                    .bar_width(4)
                    .bar_gap(1)
                    .max(100)
                    .value_style(Style::default().bg(Color::Red))
                    .label_style(Style::default().fg(Color::Cyan).modifier(Modifier::ITALIC))
                    .render(&mut f, chunks[1]);
            }
        })?;

        match events.next()? {
            Event::Input(input) => {
                if input == Key::Char('q') {
                    break;
                }
            }
            Event::Tick => {
                cpu_time_app.update(width);
            }
        }
    }

    Ok(())
}