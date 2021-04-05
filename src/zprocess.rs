/**
 * Copyright 2019-2020, Benjamin Vaisvil and the zenith contributors
 */
use crate::metrics::ProcessTableSortBy;
use heim::process;
use heim::process::ProcessError;
#[cfg(target_os = "linux")]
use libc::getpriority;
use libc::{id_t, setpriority};
use std::cmp::Ordering::{self, Equal};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use sysinfo::Process;
use sysinfo::ProcessExt;
use sysinfo::ProcessStatus;

macro_rules! convert_result_to_string {
    ($x:expr) => {
        match $x {
            Ok(_r) => String::from("Signal Sent."),
            Err(e) => convert_error_to_string!(e),
        }
    };
}

macro_rules! convert_error_to_string {
    ($x:expr) => {
        match $x {
            ProcessError::NoSuchProcess { .. } => String::from("No Such Process"),
            ProcessError::ZombieProcess { .. } => String::from("Zombie Process"),
            ProcessError::AccessDenied { .. } => String::from("Access Denied"),
            _ => String::from("Unknown error"),
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
    pub nice: i32,
    pub virtual_memory: u64,
    pub threads_total: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub prev_read_bytes: u64,
    pub prev_write_bytes: u64,
    pub last_updated: SystemTime,
    pub end_time: Option<u64>,
    pub start_time: u64,
    pub gpu_usage: u64,
    pub fb_utilization: u64,
    pub enc_utilization: u64,
    pub dec_utilization: u64,
    pub sm_utilization: u64,
}

impl ZProcess {
    pub fn from_user_and_process(user_name: String, process: &Process) -> Self {
        let disk_usage = process.disk_usage();
        ZProcess {
            uid: process.uid,
            user_name,
            pid: process.pid(),
            memory: process.memory(),
            cpu_usage: process.cpu_usage(),
            command: process.cmd().to_vec(),
            status: process.status(),
            exe: format!("{}", process.exe().display()),
            name: process.name().to_string(),
            cum_cpu_usage: process.cpu_usage() as f64,
            priority: process.priority,
            nice: process.nice,
            virtual_memory: process.virtual_memory(),
            threads_total: process.threads_total,
            read_bytes: disk_usage.total_read_bytes,
            write_bytes: disk_usage.total_written_bytes,
            prev_read_bytes: disk_usage.total_read_bytes,
            prev_write_bytes: disk_usage.total_written_bytes,
            last_updated: SystemTime::now(),
            end_time: None,
            start_time: process.start_time(),
            gpu_usage: 0,
            fb_utilization: 0,
            enc_utilization: 0,
            dec_utilization: 0,
            sm_utilization: 0,
        }
    }
    pub fn get_read_bytes_sec(&self, tick_rate: &Duration) -> f64 {
        debug!(
            "Pid {:?} Read {:?} Prev {:?}",
            self.pid, self.read_bytes, self.prev_read_bytes
        );
        (self.read_bytes - self.prev_read_bytes) as f64 / tick_rate.as_secs_f64()
    }
    pub fn get_write_bytes_sec(&self, tick_rate: &Duration) -> f64 {
        debug!(
            "Pid {:?} Write {:?} Prev {:?}",
            self.pid, self.write_bytes, self.prev_write_bytes
        );
        (self.write_bytes - self.prev_write_bytes) as f64 / tick_rate.as_secs_f64()
    }

    pub async fn suspend(&self) -> String {
        match process::get(self.pid).await {
            Ok(p) => convert_result_to_string!(p.suspend().await),
            Err(e) => convert_error_to_string!(e),
        }
    }

    pub async fn resume(&self) -> String {
        match process::get(self.pid).await {
            Ok(p) => convert_result_to_string!(p.resume().await),
            Err(e) => convert_error_to_string!(e),
        }
    }

    pub async fn kill(&self) -> String {
        match process::get(self.pid).await {
            Ok(p) => convert_result_to_string!(p.kill().await),
            Err(e) => convert_error_to_string!(e),
        }
    }

    pub async fn terminate(&self) -> String {
        match process::get(self.pid).await {
            Ok(p) => convert_result_to_string!(p.terminate().await),
            Err(e) => convert_error_to_string!(e),
        }
    }

    pub fn nice(&mut self) -> String {
        self.set_priority(19)
    }

    #[cfg(target_os = "linux")]
    pub fn set_priority(&mut self, priority: i32) -> String {
        let mut result = unsafe { setpriority(0, self.pid as id_t, priority) };

        if result < 0 {
            String::from("Couldn't set priority.")
        } else {
            unsafe {
                result = getpriority(0, self.pid as id_t);
            }
            self.priority = result + 20;
            self.nice = result;
            String::from("Priority Set.")
        }
    }

    #[cfg(target_os = "macos")]
    pub fn set_priority(&mut self, priority: i32) -> String {
        let result = unsafe { setpriority(0, self.pid as id_t, priority) };
        if result < 0 {
            String::from("Couldn't set priority.")
        } else {
            String::from("Priority Set.")
        }
    }

    pub fn set_end_time(&mut self) {
        if self.end_time.is_none() {
            self.end_time = match SystemTime::now().duration_since(UNIX_EPOCH) {
                Ok(t) => Some(t.as_secs()),
                Err(_) => panic!("System time before unix epoch??"),
            };
        }
    }

    #[cfg(not(feature = "nvidia"))]
    /// returns a pointer to a comparator function, not a closure
    pub fn field_comparator(
        sortfield: ProcessTableSortBy,
    ) -> fn(&Self, &Self, &Duration) -> Ordering {
        match sortfield {
            ProcessTableSortBy::Cpu => {
                |pa, pb, _tick| pa.cpu_usage.partial_cmp(&pb.cpu_usage).unwrap_or(Equal)
            }
            ProcessTableSortBy::Mem => |pa, pb, _tick| pa.memory.cmp(&pb.memory),
            ProcessTableSortBy::MemPerc => |pa, pb, _tick| pa.memory.cmp(&pb.memory),
            ProcessTableSortBy::User => |pa, pb, _tick| pa.user_name.cmp(&pb.user_name),
            ProcessTableSortBy::Pid => |pa, pb, _tick| pa.pid.cmp(&pb.pid),
            ProcessTableSortBy::Status => {
                |pa, pb, _tick| pa.status.to_single_char().cmp(pb.status.to_single_char())
            }
            ProcessTableSortBy::Priority => |pa, pb, _tick| pa.priority.cmp(&pb.priority),
            ProcessTableSortBy::Nice => {
                |pa, pb, _tick| pa.priority.partial_cmp(&pb.nice).unwrap_or(Equal)
            }
            ProcessTableSortBy::Virt => |pa, pb, _tick| pa.virtual_memory.cmp(&pb.virtual_memory),
            ProcessTableSortBy::Cmd => |pa, pb, _tick| pa.name.cmp(&pb.name),
            ProcessTableSortBy::DiskRead => |pa, pb, tick| {
                pa.get_read_bytes_sec(tick)
                    .partial_cmp(&pb.get_read_bytes_sec(tick))
                    .unwrap_or(Equal)
            },
            ProcessTableSortBy::DiskWrite => |pa, pb, tick| {
                pa.get_write_bytes_sec(tick)
                    .partial_cmp(&pb.get_write_bytes_sec(tick))
                    .unwrap_or(Equal)
            },
        }
    }

    #[cfg(feature = "nvidia")]
    /// returns a pointer to a comparator function, not a closure
    pub fn field_comparator(
        sortfield: ProcessTableSortBy,
    ) -> fn(&Self, &Self, &Duration) -> Ordering {
        match sortfield {
            ProcessTableSortBy::Cpu => {
                |pa, pb, _tick| pa.cpu_usage.partial_cmp(&pb.cpu_usage).unwrap_or(Equal)
            }
            ProcessTableSortBy::Mem => |pa, pb, _tick| pa.memory.cmp(&pb.memory),
            ProcessTableSortBy::MemPerc => |pa, pb, _tick| pa.memory.cmp(&pb.memory),
            ProcessTableSortBy::User => |pa, pb, _tick| pa.user_name.cmp(&pb.user_name),
            ProcessTableSortBy::Pid => |pa, pb, _tick| pa.pid.cmp(&pb.pid),
            ProcessTableSortBy::Status => {
                |pa, pb, _tick| pa.status.to_single_char().cmp(pb.status.to_single_char())
            }
            ProcessTableSortBy::Priority => |pa, pb, _tick| pa.priority.cmp(&pb.priority),
            ProcessTableSortBy::Nice => {
                |pa, pb, _tick| pa.priority.partial_cmp(&pb.nice).unwrap_or(Equal)
            }
            ProcessTableSortBy::Virt => |pa, pb, _tick| pa.virtual_memory.cmp(&pb.virtual_memory),
            ProcessTableSortBy::Cmd => |pa, pb, _tick| pa.name.cmp(&pb.name),
            ProcessTableSortBy::DiskRead => |pa, pb, tick| {
                pa.get_read_bytes_sec(tick)
                    .partial_cmp(&pb.get_read_bytes_sec(tick))
                    .unwrap_or(Equal)
            },
            ProcessTableSortBy::DiskWrite => |pa, pb, tick| {
                pa.get_write_bytes_sec(tick)
                    .partial_cmp(&pb.get_write_bytes_sec(tick))
                    .unwrap_or(Equal)
            },
            ProcessTableSortBy::Gpu => |pa, pb, _tick| pa.gpu_usage.cmp(&pb.gpu_usage),
            ProcessTableSortBy::FB => |pa, pb, _tick| pa.fb_utilization.cmp(&pb.fb_utilization),
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
