/**
 * Copyright 2019-2020, Benjamin Vaisvil and the zenith contributors
 */
use crate::restore_terminal;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde_derive::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::io::prelude::*;
use std::io::{Write};
use std::path::{Path, PathBuf};
use std::process::exit;
use std::time::{Duration, SystemTime};

const ONE_WEEK: u64 = 60 * 60 * 24 * 7;
const DB_ERROR: &str = "Couldn't open database.";
const DSER_ERROR: &str = "Couldn't deserialize object";
const SER_ERROR: &str = "Couldn't serialize object";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Histogram<'a> {
    data: Cow<'a, [u64]>,
}

impl Histogram<'_> {
    fn new(size: usize) -> Self {
        Histogram {
            data: vec![0; size].into(),
        }
    }

    pub fn data(&self) -> &[u64] {
        self.data.as_ref()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
pub enum HistogramKind {
    Cpu,
    Mem,
    NetTx,
    NetRx,
    IoRead(String),
    IoWrite(String),
    GpuUse(String),
    GpuMem(String),
    FileSystemUsedSpace(String),
}
#[derive(Clone, Copy)]
pub struct View {
    pub zoom_factor: u32,
    pub update_number: u32,
    pub width: usize,
    pub offset: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HistogramMap {
    map: HashMap<HistogramKind, Histogram<'static>>,
    duration: Duration,
    pub tick: Duration,
    db: Option<PathBuf>,
    previous_stop: Option<SystemTime>,
}

macro_rules! exit_with_message {
    ($msg:expr, $code:expr) => {
        restore_terminal();
        println!("{}", $msg);
        exit($code);
    };
}

pub fn load_zenith_store(path: &Path, current_time: &SystemTime) -> Option<HistogramMap> {
    // need to fill in time between when it was last stored and now, like the sled DB
    let mut data = std::fs::read(path).expect(DB_ERROR);
    debug!("Attempting to decompress database...");
    let mut gz = GzDecoder::new(&data[..]);
    if gz.header().is_some() {
        let mut udata = Vec::new();
        debug!("Decompressing...");
        let result = gz.read_to_end(&mut udata);
        if result.is_ok() {
            data = udata;
            debug!("Decompressed");
        }
    } else {
        debug!("Not a gzip file.");
    }
    let mut hm: HistogramMap = match bincode::deserialize(&data) {
        Ok(hm) => hm,
        Err(e) => {
            error!("{}: {}", DSER_ERROR, e,);
            return None;
        }
    };
    if let Some(previous_stop) = hm.previous_stop {
        if previous_stop < *current_time {
            let d = current_time
                .duration_since(previous_stop)
                .expect("Current time is before stored time. This should not happen.");
            let week_ticks = ONE_WEEK / hm.tick.as_secs();
            for (_k, v) in hm.map.iter_mut() {
                let data = v.data.to_mut();
                if data.len() as u64 > week_ticks {
                    let end = data.len() as u64 - week_ticks;
                    data.drain(0..end as usize);
                }
                let mut dur = d;
                // add 0s between then and now.
                let zero_dur = Duration::from_secs(0);
                while dur > zero_dur + hm.tick {
                    data.push(0);
                    dur -= hm.tick;
                }
            }
        }
    }
    Some(hm)
}

impl HistogramMap {
    pub(crate) fn new(dur: Duration, tick: Duration, db: Option<PathBuf>) -> HistogramMap {
        let current_time = SystemTime::now();
        match db {
            Some(db) => {
                debug!("Opening DB");
                let dbfile = db.join("store");
                let hm = if dbfile.exists() {
                    debug!("Zenith store exists, opening...");
                    load_zenith_store(&dbfile, &current_time)
                } else {
                    None
                }
                .unwrap_or_else(|| {
                    if let Err(e) = fs::remove_file(dbfile) {
                        error!("{}", e);
                    }
                    debug!("Starting a new database.");
                    HistogramMap {
                        map: HashMap::with_capacity(5),
                        duration: dur,
                        tick,
                        db: Some(db),
                        previous_stop: None,
                    }
                });
                hm
            }
            None => {
                debug!("Starting with no DB.");
                HistogramMap {
                    map: HashMap::with_capacity(5),
                    duration: dur,
                    tick,
                    db: None,
                    previous_stop: None,
                }
            }
        }
    }

    pub fn get_zoomed<'a>(&'a self, name: &HistogramKind, view: &View) -> Option<Histogram<'a>> {
        let h = self.get(name)?;
        let h_data = h.data();
        let h_len = h_data.len();

        if view.zoom_factor == 1 {
            let low = h_len - (view.width + view.offset);
            let high = h_len - view.offset;
            return Some(Histogram {
                data: Cow::Borrowed(&h_data[low..high]),
            });
        }

        let zf = view.zoom_factor as usize;
        let start: usize =
            h_len.saturating_sub((view.width + view.offset) * zf + view.update_number as usize);
        let end = h_len.saturating_sub(zf * view.offset);

        let new_data: Vec<_> = h_data[start..end]
            .chunks(zf)
            .map(|set| set.iter().sum::<u64>() / view.zoom_factor as u64)
            .collect();

        Some(Histogram {
            data: Cow::Owned(new_data),
        })
    }

    pub fn get(&self, name: &HistogramKind) -> Option<&Histogram> {
        self.map.get(name)
    }

    pub(crate) fn add_value_to(&mut self, name: &HistogramKind, val: u64) {
        let h = if let Some(h) = self.map.get_mut(name) {
            h
        } else {
            let size = (self.duration.as_secs() / self.tick.as_secs()) as usize; //smallest has to be >= 1000ms
            self.map
                .entry(name.clone())
                .or_insert_with(|| Histogram::new(size))
        };
        h.data.to_mut().push(val);
        debug!("Adding {} to {:?} chart.", val, name);
    }

    pub fn hist_duration(&self, width: usize, zoom_factor: u32) -> chrono::Duration {
        chrono::Duration::from_std(Duration::from_secs_f64(
            self.tick.as_secs_f64() * width as f64 * zoom_factor as f64,
        ))
        .expect("Unexpectedly large duration was out of range.")
    }

    pub fn histograms_width(&self) -> Option<usize> {
        self.map.iter().next().map(|(_k, h)| h.data.len())
    }

    pub(crate) fn save_histograms(&mut self) {
        match &self.db {
            Some(db) => {
                debug!("Saving Histograms...");
                self.previous_stop = Some(SystemTime::now());
                let dbfile = db.join("store");
                let database_open = fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(dbfile);
                match database_open {
                    Ok(database) => {
                        let mut gz = GzEncoder::new(database, Compression::default());
                        gz.write_all(&bincode::serialize(self).expect(SER_ERROR))
                            .expect("Failed to compress/write to file.");
                        match gz.finish() {
                            Ok(_r) => {
                                debug!("Write Finished.");
                            }
                            Err(_e) => {
                                error!("Couldn't complete database write.");
                            }
                        };
                        let configuration = db.join(".configuration");
                        let mut configuration = fs::OpenOptions::new()
                            .create(true)
                            .write(true)
                            .truncate(true)
                            .open(configuration)
                            .expect("Couldn't open Configuration");
                        configuration
                            .write_all(
                                format!("version={:}\n", env!("CARGO_PKG_VERSION")).as_bytes(),
                            )
                            .expect("Failed to write file.");
                    }
                    Err(e) => {
                        exit_with_message!(
                            format!(
                                "Couldn't write to {}, error: {}",
                                db.join("store").to_string_lossy(),
                                e.to_string()
                            ),
                            1
                        );
                    }
                }
            }
            None => {}
        }
    }

    pub fn writes_db_store(&self) -> bool {
        self.db.is_some()
    }
}

impl Drop for HistogramMap {
    fn drop(&mut self) {
        self.save_histograms();
    }
}
