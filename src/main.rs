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
use tui::widgets::{BarChart, Block, Borders, Widget, Sparkline};
use tui::Terminal;
use sysinfo::{NetworkExt, System, SystemExt, ProcessorExt};

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
            tick_rate: Duration::from_millis(250),
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


struct CPUTimeApp {
    cpu_usage_histogram: Vec<u64>,
    cpus: Vec<(String, u64)>,
    system: System
}

impl CPUTimeApp {
    fn new () -> CPUTimeApp{
        CPUTimeApp{
            cpu_usage_histogram: vec![],
            cpus: vec![],
            system: System::new()
        }
    }

    fn update(&mut self, width: u16) {
        self.system.refresh_all();
        let procs = self.system.get_processor_list();
        let mut num_procs = 0;
        let mut usage: f32 = 0.0;
        self.cpus.clear();
        for p in procs.iter().skip(1){
            let u = p.get_cpu_usage();
            self.cpus.push((format!("{}", num_procs), (u * 100.0) as u64));
            usage += u;
            num_procs += 1;
        }
        let usage = usage / num_procs as f32;
        self.cpu_usage_histogram.push((usage * 100.0) as u64);
        if self.cpu_usage_histogram.len() > width as usize{
            self.cpu_usage_histogram.remove(0);
        }
    }
}

struct App<'a> {
    data: Vec<(&'a str, u64)>,
}

impl<'a> App<'a> {
    fn new() -> App<'a> {
        App {
            data: vec![
                ("B1", 9),
                ("B2", 12),
                ("B3", 5),
                ("B4", 8),
                ("B5", 2),
                ("B6", 4),
                ("B7", 5),
                ("B8", 9),
                ("B9", 14),
                ("B10", 15),
                ("B11", 1),
                ("B12", 0),
                ("B13", 4),
                ("B14", 6),
                ("B15", 4),
                ("B16", 6),
                ("B17", 4),
                ("B18", 7),
                ("B19", 13),
                ("B20", 8),
                ("B21", 11),
                ("B22", 21),
                ("B23", 3),
                ("B24", 5),
            ],
        }
    }

    fn update(&mut self) {
        let value = self.data.pop().unwrap();
        self.data.insert(0, value);
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
    // App
    let mut app = App::new();

    loop {
        let mut width: u16 = 0;
        terminal.draw(|mut f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(2)
                .constraints([Constraint::Percentage(10), Constraint::Percentage(90)].as_ref())
                .split(f.size());
            width = f.size().width;
            Sparkline::default()
                .block(Block::default().title("CPU Histogram").borders(Borders::ALL))
                .data(&cpu_time_app.cpu_usage_histogram)
                .style(Style::default().fg(Color::LightBlue))
                .max(100)
                .render(&mut f, chunks[0]);
            {
                let cpus = cpu_time_app.cpus.as_slice();
                let mut xz :Vec<(&str, u64)> = vec![];
                for (p, u) in cpus.iter(){
                    xz.push((p.as_str(), u.clone()));
                }
                let chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
                    .split(chunks[1]);
                BarChart::default()
                    .block(Block::default().title("CPUS").borders(Borders::ALL))
                    .data(xz.as_slice())
                    .bar_width(5)
                    .bar_gap(3)
                    .max(100)
                    .style(Style::default().fg(Color::Green))
                    .value_style(Style::default().bg(Color::Green).modifier(Modifier::BOLD))
                    .render(&mut f, chunks[0]);
                BarChart::default()
                    .block(Block::default().title("Data3").borders(Borders::ALL))
                    .data(&app.data)
                    .style(Style::default().fg(Color::Red))
                    .bar_width(7)
                    .bar_gap(0)
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
                app.update();
                cpu_time_app.update(width);
            }
        }
    }

    Ok(())
}