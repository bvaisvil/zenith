/**
 *
 */
#[allow(dead_code)]
use crate::constants::DEFAULT_TICK;
use std::io;
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
    Save
}

#[allow(dead_code)]
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
                let mut count: u64 = 0;
                loop {
                    tx.send(Event::Tick).expect("Couldn't send event.");
                    count += 1;
                    if count % 5 == 0{
                        tx.send(Event::Save).expect("Couldn't send event");
                    }
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
