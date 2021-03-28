/**
 * Copyright 2019-2020, Benjamin Vaisvil and the zenith contributors
 */
use serde_derive::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
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
    IoRead,
    IoWrite,
    GpuUse(String),
    GpuMem(String),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HistogramMap {
    map: HashMap<HistogramKind, Histogram<'static>>,
    duration: Duration,
    pub tick: Duration,
    db: Option<PathBuf>,
    previous_stop: Option<SystemTime>,
}

pub fn load_zenith_store(path: PathBuf, current_time: &SystemTime) -> HistogramMap {
    // need to fill in time between when it was last stored and now, like the sled DB
    let data = std::fs::read(path).expect(DB_ERROR);
    let mut hm: HistogramMap = bincode::deserialize(&data).expect(DSER_ERROR);
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
    hm
}

impl HistogramMap {
    pub(crate) fn new(dur: Duration, tick: Duration, db: Option<PathBuf>) -> HistogramMap {
        let current_time = SystemTime::now();
        let path = db.as_ref().map(|db| db.to_owned());
        match &db {
            Some(db) => {
                debug!("Opening DB");
                let dbfile = Path::new(db).join(Path::new("store"));
                if dbfile.exists() {
                    debug!("Zenith store exists, opening...");
                    load_zenith_store(dbfile, &current_time)
                } else {
                    debug!("Starting a new database.");
                    HistogramMap {
                        map: HashMap::with_capacity(5),
                        duration: dur,
                        tick,
                        db: path,
                        previous_stop: None,
                    }
                }
            }
            None => {
                debug!("Starting with no DB.");
                HistogramMap {
                    map: HashMap::with_capacity(5),
                    duration: dur,
                    tick,
                    db: path,
                    previous_stop: None,
                }
            }
        }
    }

    pub fn get_zoomed<'a>(
        &'a self,
        name: &HistogramKind,
        zoom_factor: u32,
        update_number: u32,
        width: usize,
        offset: usize,
    ) -> Option<Histogram<'a>> {
        let h = self.get(name)?;
        let h_data = h.data();
        let h_len = h_data.len();

        if zoom_factor == 1 {
            let low = h_len - (width + offset);
            let high = h_len - offset;
            return Some(Histogram {
                data: Cow::Borrowed(&h_data[low..high]),
            });
        }

        let zf = zoom_factor as usize;
        let start: usize = h_len.saturating_sub((width + offset) * zf + update_number as usize);
        let end = h_len.saturating_sub(zf * offset);

        let new_data: Vec<_> = h_data[start..end]
            .chunks(zf)
            .map(|set| set.iter().sum::<u64>() / zoom_factor as u64)
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
                debug!("Saving Histograms");
                self.previous_stop = Some(SystemTime::now());
                let dbfile = Path::new(db).join(Path::new("store"));
                let mut database = fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .open(dbfile)
                    .expect("Couldn't Open DB");
                database
                    .write_all(&bincode::serialize(self).expect(SER_ERROR))
                    .expect("Failed to write file.");
                let configuration = Path::new(db).join(Path::new(".configuration"));
                let mut configuration = fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .open(configuration)
                    .expect("Couldn't open Configuration");
                configuration
                    .write_all(format!("version={:}\n", env!("CARGO_PKG_VERSION")).as_bytes())
                    .expect("Failed to write file.");
            }
            None => {}
        }
    }
}

impl Drop for HistogramMap {
    fn drop(&mut self) {
        self.save_histograms();
    }
}
