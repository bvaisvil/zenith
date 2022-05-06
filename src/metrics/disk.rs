/**
 * Copyright 2019-2020, Benjamin Vaisvil and the zenith contributors
 */
use crate::util::percent_of;
use futures::StreamExt;
use heim::disk::{io_counters, IoCounters};
use heim::units::information::byte;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::{canonicalize, read_link};
use std::ops;
use std::path::PathBuf;
use std::time::Duration;
use sysinfo::{Disk, DiskExt};

#[derive(PartialEq, Copy, Clone, Debug)]
pub struct IoMetrics {
    pub read_bytes: u64,
    pub write_bytes: u64,
}

impl IoMetrics {
    pub fn from_io_counters(io_counters: &IoCounters) -> IoMetrics {
        IoMetrics {
            read_bytes: io_counters.read_bytes().get::<byte>(),
            write_bytes: io_counters.write_bytes().get::<byte>(),
        }
    }
}

impl ops::Add for IoMetrics {
    type Output = IoMetrics;
    fn add(self, rhs: Self) -> Self {
        IoMetrics {
            read_bytes: rhs.read_bytes,
            write_bytes: rhs.write_bytes,
        }
    }
}

impl ops::AddAssign<IoMetrics> for IoMetrics {
    fn add_assign(&mut self, rhs: IoMetrics) {
        *self = IoMetrics {
            read_bytes: self.read_bytes + rhs.read_bytes,
            write_bytes: self.write_bytes + rhs.write_bytes,
        }
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct ZDisk {
    pub mount_point: PathBuf,
    pub available_bytes: u64,
    pub size_bytes: u64,
    pub name: String,
    pub file_system: String,
    pub previous_io: IoMetrics,
    pub current_io: IoMetrics,
}

impl ZDisk {
    pub fn new_total() -> ZDisk {
        let mut mock_mp = PathBuf::new();
        mock_mp.push("Total");
        ZDisk {
            mount_point: mock_mp,
            available_bytes: 0,
            size_bytes: 0,
            name: "Total".to_string(),
            file_system: "Total".to_string(),
            previous_io: IoMetrics {
                read_bytes: 0,
                write_bytes: 0,
            },
            current_io: IoMetrics {
                read_bytes: 0,
                write_bytes: 0,
            },
        }
    }

    pub fn from_disk(d: &Disk) -> ZDisk {
        ZDisk {
            mount_point: d.get_mount_point().to_path_buf(),
            available_bytes: d.get_available_space(),
            size_bytes: d.get_total_space(),
            name: get_device_name(d.get_name()),
            file_system: String::from_utf8_lossy(d.get_file_system()).into_owned(),
            previous_io: IoMetrics {
                read_bytes: 0,
                write_bytes: 0,
            },
            current_io: IoMetrics {
                read_bytes: 0,
                write_bytes: 0,
            },
        }
    }

    pub fn get_perc_free_space(&self) -> f32 {
        if self.size_bytes < 1 {
            return 0.0;
        }
        percent_of(self.available_bytes, self.size_bytes)
    }

    pub fn get_used_bytes(&self) -> u64 {
        self.size_bytes.saturating_sub(self.available_bytes)
    }

    pub fn get_read_bytes_sec(&self, tick_rate: &Duration) -> f64 {
        (self.current_io.read_bytes - self.previous_io.read_bytes) as f64 / tick_rate.as_secs_f64()
    }

    pub fn get_write_bytes_sec(&self, tick_rate: &Duration) -> f64 {
        (self.current_io.write_bytes - self.previous_io.write_bytes) as f64
            / tick_rate.as_secs_f64()
    }

    pub fn get_perc_used_space(&self) -> f32 {
        if self.size_bytes < 1 {
            0.0
        } else {
            percent_of(self.get_used_bytes(), self.size_bytes)
        }
    }
}

pub fn get_device_name(dev: &OsStr) -> String {
    // adapted from https://github.com/ClementTsang/bottom/blob/master/src/app/data_harvester/disks/heim/linux.rs
    if let Ok(path) = read_link(dev) {
        if path.is_absolute() {
            path.to_string_lossy().to_string()
        } else {
            let mut np = PathBuf::new();
            np.push(&dev);
            np.pop();
            np.push(path);
            if let Ok(cp) = canonicalize(np) {
                cp.to_string_lossy().to_string()
            } else {
                dev.to_string_lossy().to_string()
            }
        }
    } else {
        dev.to_string_lossy().to_string()
    }
}

pub async fn get_disk_io_metrics(disks: &mut HashMap<String, ZDisk>) {
    let io_counters_fut = io_counters().await;

    if let Ok(io_counter_stream) = io_counters_fut {
        ::futures::pin_mut!(io_counter_stream);
        while let Some(i) = io_counter_stream.next().await {
            if let Ok(i) = i {
                let name = i.device_name().to_string_lossy().to_string();
                debug!("Name: {:}", name);
                if let Some(d) = disks.get_mut(&format!("/dev/{:}", name)) {
                    let io_metrics = IoMetrics::from_io_counters(&i);
                    d.previous_io = d.current_io;
                    d.current_io = io_metrics;
                    if d.previous_io.write_bytes == 0 && d.previous_io.read_bytes == 0 {
                        d.previous_io.write_bytes = d.current_io.write_bytes;
                        d.previous_io.read_bytes = d.current_io.read_bytes;
                    }
                    debug!("{:?}", d);
                } else {
                }
            } else {
                debug!("Couldn't get counters for a disk, skipping.")
            }
        }
    }
}
