/**
 * Copyright 2019 Benjamin Vaisvil
 */
use crate::constants::DEFAULT_TICK;
use heim::process;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use heim::process::{ProcessError};

use sysinfo::ProcessStatus;

macro_rules! convert_result_to_string{
    ($x:expr) => {
        match $x{
            Ok(r) => String::from("Signal Sent."),
            Err(e) => convert_error_to_string!(e)
        }
    };
}

macro_rules! convert_error_to_string {
    ($x:expr) => {
        match $x{
            ProcessError::NoSuchProcess {..} => String::from("No Such Process"),
            ProcessError::ZombieProcess {..} => String::from("Zombie Process"),
            ProcessError::AccessDenied {..} => String::from("Access Denied"),
            _ => String::from("Unknow error")
        }
    };
}

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

    pub async fn suspend(&self) -> String{
        match process::get(self.pid).await{
            Ok(p) => convert_result_to_string!(p.suspend().await),
            Err(e) => convert_error_to_string!(e)
        }
    }

    pub async fn resume(&self) -> String {
        match process::get(self.pid).await{
            Ok(p) => convert_result_to_string!(p.resume().await),
            Err(e) => convert_error_to_string!(e)
        }
    }

    pub async fn kill(&self)  -> String{
        match process::get(self.pid).await{
            Ok(p) => convert_result_to_string!(p.kill().await),
            Err(e) => convert_error_to_string!(e)
        }
    }

    pub async fn terminate(&self)  -> String{
        match process::get(self.pid).await{
            Ok(p) => convert_result_to_string!(p.terminate().await),
            Err(e) => convert_error_to_string!(e)
        }
    }

    pub fn set_end_time(&mut self){
        if self.end_time.is_none(){
            self.end_time = match SystemTime::now().duration_since(UNIX_EPOCH) {
                Ok(t) => Some(t.as_secs()),
                Err(_) => panic!("System time before unix epoch??"),
            };
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
