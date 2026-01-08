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
                Signals::new([SIGINT, SIGTERM, SIGABRT]).expect("Couldn't create signal handler");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyCode as Key;
    use std::time::Duration;

    // Tests for percent_of
    #[test]
    fn test_percent_of_zero_numerator() {
        assert_eq!(percent_of(0, 100), 0.0);
    }

    #[test]
    fn test_percent_of_zero_denominator() {
        assert_eq!(percent_of(100, 0), 0.0);
    }

    #[test]
    fn test_percent_of_both_zero() {
        assert_eq!(percent_of(0, 0), 0.0);
    }

    #[test]
    fn test_percent_of_fifty_percent() {
        assert_eq!(percent_of(50, 100), 50.0);
    }

    #[test]
    fn test_percent_of_hundred_percent() {
        assert_eq!(percent_of(100, 100), 100.0);
    }

    #[test]
    fn test_percent_of_quarter() {
        assert_eq!(percent_of(25, 100), 25.0);
    }

    #[test]
    fn test_percent_of_over_hundred() {
        // When numerator > denominator
        assert_eq!(percent_of(200, 100), 200.0);
    }

    #[test]
    fn test_percent_of_small_values() {
        let result = percent_of(1, 4);
        assert!((result - 25.0).abs() < 0.01);
    }

    #[test]
    fn test_percent_of_large_values() {
        let result = percent_of(500_000_000, 1_000_000_000);
        assert!((result - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_percent_of_precise() {
        let result = percent_of(1, 3);
        // 1/3 * 100 = 33.333...
        assert!((result - 33.333).abs() < 0.01);
    }

    // Tests for Config
    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.exit_key, Key::Char('q'));
        assert_eq!(
            config.tick_rate,
            Duration::from_millis(crate::constants::DEFAULT_TICK)
        );
    }

    #[test]
    fn test_config_custom() {
        let config = Config {
            exit_key: Key::Esc,
            tick_rate: Duration::from_millis(1000),
        };
        assert_eq!(config.exit_key, Key::Esc);
        assert_eq!(config.tick_rate, Duration::from_millis(1000));
    }

    #[test]
    fn test_config_clone() {
        let config1 = Config::default();
        let config2 = config1.clone();
        assert_eq!(config1.exit_key, config2.exit_key);
        assert_eq!(config1.tick_rate, config2.tick_rate);
    }

    #[test]
    fn test_config_copy() {
        let config1 = Config::default();
        let config2 = config1;
        assert_eq!(config1.exit_key, config2.exit_key);
        assert_eq!(config1.tick_rate, config2.tick_rate);
    }

    #[test]
    fn test_config_debug() {
        let config = Config::default();
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("exit_key"));
        assert!(debug_str.contains("tick_rate"));
    }

    // Tests for Event enum
    #[test]
    fn test_event_tick() {
        let event: Event<()> = Event::Tick;
        match event {
            Event::Tick => assert!(true),
            _ => panic!("Expected Tick event"),
        }
    }

    #[test]
    fn test_event_save() {
        let event: Event<()> = Event::Save;
        match event {
            Event::Save => assert!(true),
            _ => panic!("Expected Save event"),
        }
    }

    #[test]
    fn test_event_terminate() {
        let event: Event<()> = Event::Terminate;
        match event {
            Event::Terminate => assert!(true),
            _ => panic!("Expected Terminate event"),
        }
    }

    #[test]
    fn test_event_resize() {
        let event: Event<()> = Event::Resize(80, 24);
        match event {
            Event::Resize(cols, rows) => {
                assert_eq!(cols, 80);
                assert_eq!(rows, 24);
            }
            _ => panic!("Expected Resize event"),
        }
    }

    #[test]
    fn test_event_input() {
        let event: Event<i32> = Event::Input(42);
        match event {
            Event::Input(val) => assert_eq!(val, 42),
            _ => panic!("Expected Input event"),
        }
    }

    // Edge case tests for percent_of
    #[test]
    fn test_percent_of_one_byte() {
        let result = percent_of(1, 1);
        assert_eq!(result, 100.0);
    }

    #[test]
    fn test_percent_of_max_u64() {
        // Testing with large values close to u64 max
        let large = u64::MAX / 2;
        let result = percent_of(large, u64::MAX);
        // Should be approximately 50%
        assert!((result - 50.0).abs() < 1.0);
    }
}
