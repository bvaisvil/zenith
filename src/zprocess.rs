/**
 * Copyright 2019-2020, Benjamin Vaisvil and the zenith contributors
 */
use crate::constants::DEFAULT_TICK;
use crate::metrics::ProcessTableSortBy;
use heim::process;
use heim::process::ProcessError;
#[cfg(target_os = "linux")]
use libc::getpriority;
use libc::{id_t, setpriority};
use std::cmp::Ordering::{self, Equal};
use std::time::{SystemTime, UNIX_EPOCH};
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
    pub fn get_read_bytes_sec(&self) -> f64 {
        debug!(
            "Pid {:?} Read {:?} Prev {:?}",
            self.pid, self.read_bytes, self.prev_read_bytes
        );
        (self.read_bytes - self.prev_read_bytes) as f64 / (DEFAULT_TICK as f64 / 1000.0)
    }
    pub fn get_write_bytes_sec(&self) -> f64 {
        debug!(
            "Pid {:?} Write {:?} Prev {:?}",
            self.pid, self.write_bytes, self.prev_write_bytes
        );
        (self.write_bytes - self.prev_write_bytes) as f64 / (DEFAULT_TICK as f64 / 1000.0)
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
    pub fn field_comparator(sortfield: ProcessTableSortBy) -> fn(&Self, &Self) -> Ordering {
        match sortfield {
            ProcessTableSortBy::CPU => {
                |pa, pb| pa.cpu_usage.partial_cmp(&pb.cpu_usage).unwrap_or(Equal)
            }
            ProcessTableSortBy::Mem => |pa, pb| pa.memory.cmp(&pb.memory),
            ProcessTableSortBy::MemPerc => |pa, pb| pa.memory.cmp(&pb.memory),
            ProcessTableSortBy::User => |pa, pb| pa.user_name.cmp(&pb.user_name),
            ProcessTableSortBy::Pid => |pa, pb| pa.pid.cmp(&pb.pid),
            ProcessTableSortBy::Status => {
                |pa, pb| pa.status.to_single_char().cmp(pb.status.to_single_char())
            }
            ProcessTableSortBy::Priority => |pa, pb| pa.priority.cmp(&pb.priority),
            ProcessTableSortBy::Nice => |pa, pb| pa.priority.partial_cmp(&pb.nice).unwrap_or(Equal),
            ProcessTableSortBy::Virt => |pa, pb| pa.virtual_memory.cmp(&pb.virtual_memory),
            ProcessTableSortBy::Cmd => |pa, pb| pa.name.cmp(&pb.name),
            ProcessTableSortBy::DiskRead => |pa, pb| {
                pa.get_read_bytes_sec()
                    .partial_cmp(&pb.get_read_bytes_sec())
                    .unwrap_or(Equal)
            },
            ProcessTableSortBy::DiskWrite => |pa, pb| {
                pa.get_write_bytes_sec()
                    .partial_cmp(&pb.get_write_bytes_sec())
                    .unwrap_or(Equal)
            },
        }
    }

    #[cfg(feature = "nvidia")]
    /// returns a pointer to a comparator function, not a closure
    pub fn field_comparator(sortfield: ProcessTableSortBy) -> fn(&Self, &Self) -> Ordering {
        match sortfield {
            ProcessTableSortBy::CPU => {
                |pa, pb| pa.cpu_usage.partial_cmp(&pb.cpu_usage).unwrap_or(Equal)
            }
            ProcessTableSortBy::Mem => |pa, pb| pa.memory.cmp(&pb.memory),
            ProcessTableSortBy::MemPerc => |pa, pb| pa.memory.cmp(&pb.memory),
            ProcessTableSortBy::User => |pa, pb| pa.user_name.cmp(&pb.user_name),
            ProcessTableSortBy::Pid => |pa, pb| pa.pid.cmp(&pb.pid),
            ProcessTableSortBy::Status => {
                |pa, pb| pa.status.to_single_char().cmp(pb.status.to_single_char())
            }
            ProcessTableSortBy::Priority => |pa, pb| pa.priority.cmp(&pb.priority),
            ProcessTableSortBy::Nice => |pa, pb| pa.priority.partial_cmp(&pb.nice).unwrap_or(Equal),
            ProcessTableSortBy::Virt => |pa, pb| pa.virtual_memory.cmp(&pb.virtual_memory),
            ProcessTableSortBy::Cmd => |pa, pb| pa.name.cmp(&pb.name),
            ProcessTableSortBy::DiskRead => |pa, pb| {
                pa.get_read_bytes_sec()
                    .partial_cmp(&pb.get_read_bytes_sec())
                    .unwrap_or(Equal)
            },
            ProcessTableSortBy::DiskWrite => |pa, pb| {
                pa.get_write_bytes_sec()
                    .partial_cmp(&pb.get_write_bytes_sec())
                    .unwrap_or(Equal)
            },
            ProcessTableSortBy::GPU => |pa, pb| pa.gpu_usage.cmp(&pb.gpu_usage),
            ProcessTableSortBy::FB => |pa, pb| pa.fb_utilization.cmp(&pb.fb_utilization),
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
