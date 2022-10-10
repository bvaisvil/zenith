#![allow(dead_code)]
/**
 * Copyright 2019-2020, Benjamin Vaisvil and the zenith contributors
 */
use crate::constants::DEFAULT_TICK;
use crossterm::{event, event::Event as CEvent, event::KeyCode as Key, event::KeyEvent};
use signal_hook::consts::signal::{SIGABRT, SIGINT, SIGTERM};
use signal_hook::iterator::Signals;
use std::fs::{remove_file, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

pub enum Event<I> {
    Input(I),
    Resize(u16, u16),
    Tick,
    Save,
    Terminate,
}

#[allow(dead_code)]
pub struct Events {
    rx: mpsc::Receiver<Event<KeyEvent>>,
    input_handle: thread::JoinHandle<()>,
    tick_handle: thread::JoinHandle<()>,
    sig_handle: thread::JoinHandle<()>,
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
            tick_rate: Duration::from_millis(DEFAULT_TICK),
        }
    }
}

impl Events {
    pub fn new(tick_rate: Duration) -> Events {
        Events::with_config(Config {
            tick_rate,
            exit_key: Key::Char('q'),
        })
    }

    pub fn with_config(config: Config) -> Events {
        let (tx, rx) = mpsc::channel();
        let input_handle = {
            let tx = tx.clone();
            thread::spawn(move || loop {
                match event::read().expect("Couldn't read event") {
                    CEvent::Key(key) => tx.send(Event::Input(key)).expect("Couldn't send event."),
                    CEvent::Resize(cols, rows) => tx
                        .send(Event::Resize(cols, rows))
                        .expect("Couldn't send event."),
                    _ => (), // ignore
                }
            })
        };
        let tick_handle = {
            let tx = tx.clone();
            thread::spawn(move || {
                let tx = tx.clone();
                let mut count: u64 = 0;
                loop {
                    tx.send(Event::Tick).expect("Couldn't send event.");
                    count += 1;
                    if count % 60 == 0 {
                        tx.send(Event::Save).expect("Couldn't send event");
                    }
                    thread::sleep(config.tick_rate);
                }
            })
        };
        let sig_handle = {
            let tx = tx;
            let mut signals =
                Signals::new(&[SIGINT, SIGTERM, SIGABRT]).expect("Couldn't create signal handler");
            thread::spawn(move || {
                let tx = tx.clone();
                for _sig in signals.forever() {
                    tx.send(Event::Terminate)
                        .expect("Couldn't send Terminate event.");
                }
            })
        };
        Events {
            rx,
            input_handle,
            tick_handle,
            sig_handle,
        }
    }

    pub fn next(&self) -> Result<Event<KeyEvent>, mpsc::RecvError> {
        self.rx.recv()
    }
}

/// Keeps a file open exclusively
/// Removes the file when dropped
pub struct Lockfile {
    file: File,
    path: PathBuf,
}

impl Lockfile {
    /// Tries to open the file creating if it does not exist
    /// Fails if zenith is already running using the same lockfile
    pub async fn new(main_pid: u32, path: &Path) -> Option<Self> {
        if is_zenith_running(path).await {
            debug!("{}", path.to_string_lossy());
            return None;
        }

        let mut file = File::create(path).ok()?;

        file.write_all(main_pid.to_string().as_bytes()).ok()?;

        Some(Self {
            file,
            path: path.into(),
        })
    }
}

impl Drop for Lockfile {
    fn drop(&mut self) {
        debug!("Removing Lock");
        let res = remove_file(&self.path);
        if let Err(e) = res {
            error!(
                "Error deleting lockfile: path={}, error={:?}",
                self.path.display(),
                e
            );
        }
    }
}

async fn is_zenith_running(path: &Path) -> bool {
    name_of_process_for_pidfile(path)
        .await
        .map_or(false, |name| name == "zenith")
}

async fn name_of_process_for_pidfile(path: &Path) -> Option<String> {
    let data = std::fs::read_to_string(path).ok()?;
    let pid: i32 = data.parse().ok()?;
    let process = heim::process::get(pid).await.ok()?;
    process.name().await.ok()
}

pub fn percent_of(numerator: u64, denominator: u64) -> f32 {
    if numerator == 0 || denominator == 0 {
        0.0
    } else {
        (numerator as f32 / denominator as f32) * 100.0
    }
}
