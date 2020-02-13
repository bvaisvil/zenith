/**
 * Copyright 2019 Benjamin Vaisvil
 */
use crate::constants::DEFAULT_TICK;
use heim::process;
use std::time::SystemTime;

use sysinfo::ProcessStatus;

#[derive(Clone)]
pub struct ZProcess {
    pub pid: i32,
    pub uid: u32,
    pub user_name: String,
    pub memory: u64,
    pub cpu_usage: f32,
    pub cum_cpu_usage: f64,
    pub command: Vec<String>,
    pub exe: String,
    pub status: ProcessStatus,
    pub name: String,
    pub priority: i32,
    pub virtual_memory: u64,
    pub threads_total: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub prev_read_bytes: u64,
    pub prev_write_bytes: u64,
    pub last_updated: SystemTime,
    pub end_time: Option<u64>,
    pub start_time: u64,
}

impl ZProcess {
    pub fn get_read_bytes_sec(&self) -> f64 {
        (self.read_bytes - self.prev_read_bytes) as f64 / (DEFAULT_TICK as f64 / 1000.0)
    }
    pub fn get_write_bytes_sec(&self) -> f64 {
        (self.write_bytes - self.prev_write_bytes) as f64 / (DEFAULT_TICK as f64 / 1000.0)
    }

    pub async fn suspend(&self) {
        let p = process::get(self.pid).await.ok();

        if p.is_some() {
            p.unwrap().suspend().await.ok();
        }
    }

    pub async fn resume(&self) {
        let p = process::get(self.pid).await.ok();
        if p.is_some() {
            p.unwrap().resume().await.ok();
        }
    }

    pub async fn kill(&self) {
        let p = process::get(self.pid).await.ok();
        if p.is_some() {
            p.unwrap().kill().await.ok();
        }
    }

    pub async fn terminate(&self) {
        let p = process::get(self.pid).await.ok();
        if p.is_some() {
            p.unwrap().terminate().await.ok();
        }
    }
}

pub trait ProcessStatusExt {
    fn to_single_char(&self) -> &str;
}

impl ProcessStatusExt for ProcessStatus {
    #[cfg(target_os = "macos")]
    fn to_single_char(&self) -> &str {
        match *self {
            ProcessStatus::Idle => "I",
            ProcessStatus::Run => "R",
            ProcessStatus::Sleep => "S",
            ProcessStatus::Stop => "T",
            ProcessStatus::Zombie => "Z",
            ProcessStatus::Unknown(_) => "U",
        }
    }

    #[cfg(all(any(unix), not(target_os = "macos")))]
    fn to_single_char(&self) -> &str {
        match *self {
            ProcessStatus::Idle => "I",
            ProcessStatus::Run => "R",
            ProcessStatus::Sleep => "S",
            ProcessStatus::Stop => "T",
            ProcessStatus::Zombie => "Z",
            ProcessStatus::Tracing => "t",
            ProcessStatus::Dead => "x",
            ProcessStatus::Wakekill => "K",
            ProcessStatus::Waking => "W",
            ProcessStatus::Parked => "P",
            ProcessStatus::Unknown(_) => "U",
        }
    }
}
