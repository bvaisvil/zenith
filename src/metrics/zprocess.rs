/**
 * Copyright 2019-2020, Benjamin Vaisvil and the zenith contributors
 */
use crate::metrics::ProcessTableSortBy;
use heim::process;
use heim::process::ProcessError;
#[cfg(target_os = "linux")]
use libc::getpriority;
use libc::{id_t, setpriority};

#[cfg(target_os = "linux")]
use linux_taskstats::Client;

use std::cmp::Ordering::{self};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use sysinfo::Process;
use sysinfo::ProcessExt;
use sysinfo::ProcessStatus;

use chrono::prelude::DateTime;
use chrono::Duration as CDuration;
use chrono::Local;

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

#[allow(dead_code)]
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
    pub io_delay: Duration,
    pub swap_delay: Duration,
    pub prev_io_delay: Duration,
    pub prev_swap_delay: Duration,
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
            io_delay: Duration::from_nanos(0),
            swap_delay: Duration::from_nanos(0),
            prev_io_delay: Duration::from_nanos(0),
            prev_swap_delay: Duration::from_nanos(0),
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

    pub fn get_run_duration(&self) -> CDuration {
        let start_time = DateTime::<Local>::from(UNIX_EPOCH + Duration::from_secs(self.start_time));
        let et = match self.end_time {
            Some(t) => DateTime::<Local>::from(UNIX_EPOCH + Duration::from_secs(t)),
            None => Local::now(),
        };
        et - start_time
    }

    #[cfg(target_os = "linux")]
    pub fn get_io_wait(&self, tick_rate: &Duration) -> f64 {
        ((self.io_delay.as_secs_f64() - self.prev_io_delay.as_secs_f64()) / tick_rate.as_secs_f64())
            * 100.0
    }

    #[cfg(target_os = "linux")]
    pub fn get_total_io_wait(&self) -> f64 {
        let process_duration = self
            .get_run_duration()
            .to_std()
            .expect("Duration out of expected range!");
        (self.io_delay.as_secs_f64() / process_duration.as_secs_f64()) * 100.0
    }

    #[cfg(target_os = "linux")]
    pub fn get_swap_wait(&self, tick_rate: &Duration) -> f64 {
        ((self.swap_delay.as_secs_f64() - self.prev_swap_delay.as_secs_f64())
            / tick_rate.as_secs_f64())
            * 100.0
    }

    #[cfg(target_os = "linux")]
    pub fn get_total_swap_wait(&self) -> f64 {
        let process_duration = self
            .get_run_duration()
            .to_std()
            .expect("Duration out of expected range!");
        (self.swap_delay.as_secs_f64() / process_duration.as_secs_f64()) * 100.0
    }

    #[cfg(target_os = "linux")]
    pub fn update_delay(&mut self, client: &Option<Client>) {
        debug!("Getting Task Stats for {}", self.pid);
        if let Some(c) = client {
            let stats_result = c.pid_stats(self.pid as u32);
            match stats_result {
                Ok(s) => {
                    self.prev_io_delay = self.io_delay;
                    self.prev_swap_delay = self.swap_delay;
                    self.io_delay = s.delays.blkio.delay_total;
                    self.swap_delay = s.delays.swapin.delay_total;
                    debug!(
                        "Pid: {} io_delay: {} swap_delay: {}",
                        self.pid,
                        self.io_delay.as_secs(),
                        self.swap_delay.as_secs()
                    );
                }
                Err(_) => debug!("Couldn't get stats for {}", self.pid),
            }
        }
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

    #[cfg(all(not(feature = "nvidia"), not(target_os = "linux")))]
    /// returns a pointer to a comparator function, not a closure
    pub fn field_comparator(
        sortfield: ProcessTableSortBy,
    ) -> fn(&Self, &Self, &Duration) -> Ordering {
        match sortfield {
            ProcessTableSortBy::Cpu => |pa, pb, _tick| pa.cpu_usage.total_cmp(&pb.cpu_usage),
            ProcessTableSortBy::Mem => |pa, pb, _tick| pa.memory.cmp(&pb.memory),
            ProcessTableSortBy::MemPerc => |pa, pb, _tick| pa.memory.cmp(&pb.memory),
            ProcessTableSortBy::User => |pa, pb, _tick| pa.user_name.cmp(&pb.user_name),
            ProcessTableSortBy::Pid => |pa, pb, _tick| pa.pid.cmp(&pb.pid),
            ProcessTableSortBy::Status => {
                |pa, pb, _tick| pa.status.to_single_char().cmp(pb.status.to_single_char())
            }
            ProcessTableSortBy::Priority => |pa, pb, _tick| pa.priority.cmp(&pb.priority),
            ProcessTableSortBy::Nice => |pa, pb, _tick| pa.nice.cmp(&pb.nice),
            ProcessTableSortBy::Virt => |pa, pb, _tick| pa.virtual_memory.cmp(&pb.virtual_memory),
            ProcessTableSortBy::Cmd => |pa, pb, _tick| pa.name.cmp(&pb.name),
            ProcessTableSortBy::DiskRead => |pa, pb, tick| {
                pa.get_read_bytes_sec(tick)
                    .total_cmp(&pb.get_read_bytes_sec(tick))
            },
            ProcessTableSortBy::DiskWrite => |pa, pb, tick| {
                pa.get_write_bytes_sec(tick)
                    .total_cmp(&pb.get_write_bytes_sec(tick))
            },
        }
    }

    #[cfg(all(feature = "nvidia", not(target_os = "linux")))]
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

    #[cfg(all(not(feature = "nvidia"), target_os = "linux"))]
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
            ProcessTableSortBy::IOWait => |pa, pb, _tick| {
                (pa.io_delay - pa.prev_io_delay).cmp(&(pb.io_delay - pb.prev_io_delay))
            },
        }
    }

    #[cfg(all(feature = "nvidia", target_os = "linux"))]
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
            ProcessTableSortBy::IOWait => |pa, pb, _tick| {
                (pa.io_delay - pa.prev_io_delay).cmp(&(pb.io_delay - pb.prev_io_delay))
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

    #[cfg(all(unix, not(target_os = "macos")))]
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn create_test_process() -> ZProcess {
        ZProcess {
            pid: 1234,
            uid: 1000,
            user_name: "testuser".to_string(),
            memory: 1024 * 1024,
            cpu_usage: 50.0,
            cum_cpu_usage: 100.0,
            command: vec!["test".to_string(), "--arg".to_string()],
            exe: "/usr/bin/test".to_string(),
            status: ProcessStatus::Run,
            name: "test_process".to_string(),
            priority: 20,
            nice: 0,
            virtual_memory: 2 * 1024 * 1024,
            threads_total: 4,
            read_bytes: 1000,
            write_bytes: 500,
            prev_read_bytes: 500,
            prev_write_bytes: 250,
            last_updated: SystemTime::now(),
            end_time: None,
            start_time: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                - 3600, // Started 1 hour ago
            gpu_usage: 0,
            fb_utilization: 0,
            enc_utilization: 0,
            dec_utilization: 0,
            sm_utilization: 0,
            io_delay: Duration::from_nanos(0),
            swap_delay: Duration::from_nanos(0),
            prev_io_delay: Duration::from_nanos(0),
            prev_swap_delay: Duration::from_nanos(0),
        }
    }

    #[test]
    fn test_get_read_bytes_sec() {
        let process = create_test_process();
        let tick_rate = Duration::from_secs(1);
        
        // read_bytes: 1000, prev_read_bytes: 500
        // (1000 - 500) / 1.0 = 500.0
        let read_rate = process.get_read_bytes_sec(&tick_rate);
        assert!((read_rate - 500.0).abs() < 0.01);
    }

    #[test]
    fn test_get_read_bytes_sec_with_different_tick() {
        let process = create_test_process();
        let tick_rate = Duration::from_secs(2);
        
        // (1000 - 500) / 2.0 = 250.0
        let read_rate = process.get_read_bytes_sec(&tick_rate);
        assert!((read_rate - 250.0).abs() < 0.01);
    }

    #[test]
    fn test_get_write_bytes_sec() {
        let process = create_test_process();
        let tick_rate = Duration::from_secs(1);
        
        // write_bytes: 500, prev_write_bytes: 250
        // (500 - 250) / 1.0 = 250.0
        let write_rate = process.get_write_bytes_sec(&tick_rate);
        assert!((write_rate - 250.0).abs() < 0.01);
    }

    #[test]
    fn test_get_write_bytes_sec_with_different_tick() {
        let process = create_test_process();
        let tick_rate = Duration::from_millis(500);
        
        // (500 - 250) / 0.5 = 500.0
        let write_rate = process.get_write_bytes_sec(&tick_rate);
        assert!((write_rate - 500.0).abs() < 0.01);
    }

    #[test]
    fn test_get_run_duration_alive_process() {
        let process = create_test_process();
        let duration = process.get_run_duration();
        
        // Process started 1 hour ago
        assert!(duration.num_minutes() >= 59);
        assert!(duration.num_minutes() <= 61);
    }

    #[test]
    fn test_get_run_duration_dead_process() {
        let mut process = create_test_process();
        // Process ended 30 minutes after start
        process.end_time = Some(process.start_time + 1800);
        
        let duration = process.get_run_duration();
        assert_eq!(duration.num_minutes(), 30);
    }

    #[test]
    fn test_set_end_time() {
        let mut process = create_test_process();
        assert!(process.end_time.is_none());
        
        process.set_end_time();
        assert!(process.end_time.is_some());
        
        // Calling again should not change the end time
        let first_end_time = process.end_time;
        process.set_end_time();
        assert_eq!(process.end_time, first_end_time);
    }

    #[test]
    fn test_field_comparator_cpu() {
        let mut p1 = create_test_process();
        let mut p2 = create_test_process();
        p1.cpu_usage = 10.0;
        p2.cpu_usage = 20.0;
        
        let tick = Duration::from_secs(1);
        let comparator = ZProcess::field_comparator(ProcessTableSortBy::Cpu);
        
        assert_eq!(comparator(&p1, &p2, &tick), Ordering::Less);
        assert_eq!(comparator(&p2, &p1, &tick), Ordering::Greater);
        assert_eq!(comparator(&p1, &p1, &tick), Ordering::Equal);
    }

    #[test]
    fn test_field_comparator_memory() {
        let mut p1 = create_test_process();
        let mut p2 = create_test_process();
        p1.memory = 1000;
        p2.memory = 2000;
        
        let tick = Duration::from_secs(1);
        let comparator = ZProcess::field_comparator(ProcessTableSortBy::Mem);
        
        assert_eq!(comparator(&p1, &p2, &tick), Ordering::Less);
        assert_eq!(comparator(&p2, &p1, &tick), Ordering::Greater);
    }

    #[test]
    fn test_field_comparator_pid() {
        let mut p1 = create_test_process();
        let mut p2 = create_test_process();
        p1.pid = 100;
        p2.pid = 200;
        
        let tick = Duration::from_secs(1);
        let comparator = ZProcess::field_comparator(ProcessTableSortBy::Pid);
        
        assert_eq!(comparator(&p1, &p2, &tick), Ordering::Less);
    }

    #[test]
    fn test_field_comparator_user() {
        let mut p1 = create_test_process();
        let mut p2 = create_test_process();
        p1.user_name = "alice".to_string();
        p2.user_name = "bob".to_string();
        
        let tick = Duration::from_secs(1);
        let comparator = ZProcess::field_comparator(ProcessTableSortBy::User);
        
        assert_eq!(comparator(&p1, &p2, &tick), Ordering::Less);
    }

    #[test]
    fn test_field_comparator_disk_read() {
        let mut p1 = create_test_process();
        let mut p2 = create_test_process();
        p1.read_bytes = 1000;
        p1.prev_read_bytes = 0;
        p2.read_bytes = 2000;
        p2.prev_read_bytes = 0;
        
        let tick = Duration::from_secs(1);
        let comparator = ZProcess::field_comparator(ProcessTableSortBy::DiskRead);
        
        assert_eq!(comparator(&p1, &p2, &tick), Ordering::Less);
    }

    #[test]
    fn test_field_comparator_cmd() {
        let mut p1 = create_test_process();
        let mut p2 = create_test_process();
        p1.name = "aaa".to_string();
        p2.name = "zzz".to_string();
        
        let tick = Duration::from_secs(1);
        let comparator = ZProcess::field_comparator(ProcessTableSortBy::Cmd);
        
        assert_eq!(comparator(&p1, &p2, &tick), Ordering::Less);
    }

    #[test]
    fn test_process_status_to_single_char() {
        assert_eq!(ProcessStatus::Run.to_single_char(), "R");
        assert_eq!(ProcessStatus::Sleep.to_single_char(), "S");
        assert_eq!(ProcessStatus::Stop.to_single_char(), "T");
        assert_eq!(ProcessStatus::Zombie.to_single_char(), "Z");
        assert_eq!(ProcessStatus::Idle.to_single_char(), "I");
    }

    #[test]
    fn test_zero_io_rates() {
        let mut process = create_test_process();
        process.read_bytes = 0;
        process.prev_read_bytes = 0;
        process.write_bytes = 0;
        process.prev_write_bytes = 0;
        
        let tick_rate = Duration::from_secs(1);
        
        assert_eq!(process.get_read_bytes_sec(&tick_rate), 0.0);
        assert_eq!(process.get_write_bytes_sec(&tick_rate), 0.0);
    }

    #[test]
    fn test_large_io_rates() {
        let mut process = create_test_process();
        process.read_bytes = 1_000_000_000; // 1GB
        process.prev_read_bytes = 0;
        
        let tick_rate = Duration::from_secs(1);
        let rate = process.get_read_bytes_sec(&tick_rate);
        
        assert!((rate - 1_000_000_000.0).abs() < 1.0);
    }
}

