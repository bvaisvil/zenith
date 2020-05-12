#![allow(dead_code)]
/**
 *
 */
use crate::constants::DEFAULT_TICK;
use signal_hook::{iterator::Signals, SIGABRT, SIGINT, SIGTERM};
use std::fs::{remove_file, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use termion::event::Key;
use termion::input::TermRead;

// pub struct TabsState<'a> {
//     pub titles: Vec<&'a str>,
//     pub index: usize,
// }

// impl<'a> TabsState<'a> {
//     pub fn new(titles: Vec<&'a str>) -> TabsState {
//         TabsState { titles, index: 0 }
//     }
//     pub fn next(&mut self) {
//         self.index = (self.index + 1) % self.titles.len();
//     }

//     pub fn previous(&mut self) {
//         if self.index > 0 {
//             self.index -= 1;
//         } else {
//             self.index = self.titles.len() - 1;
//         }
//     }
// }

pub enum Event<I> {
    Input(I),
    Tick,
    Save,
    Terminate,
}

#[allow(dead_code)]
pub struct Events {
    rx: mpsc::Receiver<Event<Key>>,
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
    pub fn new(tick_rate: u64) -> Events {
        Events::with_config(Config {
            tick_rate: Duration::from_millis(tick_rate),
            exit_key: Key::Char('q'),
        })
    }

    pub fn with_config(config: Config) -> Events {
        let (tx, rx) = mpsc::channel();
        let input_handle = {
            let tx = tx.clone();
            thread::spawn(move || {
                let stdin = io::stdin();
                for evt in stdin.keys() {
                    if let Ok(key) = evt {
                        if tx.send(Event::Input(key)).is_err() {
                            return;
                        }
                    }
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
            let signals =
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

    pub fn next(&self) -> Result<Event<Key>, mpsc::RecvError> {
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
