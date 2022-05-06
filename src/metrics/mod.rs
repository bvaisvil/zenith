pub mod disk;
/**
 * Copyright 2019-2020, Benjamin Vaisvil and the zenith contributors
 */
pub mod graphics;
pub mod histogram;
pub mod zprocess;

use crate::metrics::disk::{get_device_name, get_disk_io_metrics, ZDisk, IoMetrics};
use crate::metrics::graphics::device::{GraphicsDevice, GraphicsExt};
use crate::metrics::histogram::{HistogramKind, HistogramMap};
use crate::metrics::zprocess::ZProcess;
use crate::util::percent_of;

use futures::StreamExt;
use heim::host;
use heim::net;
use heim::net::Address;
use heim::units::frequency::megahertz;
use heim::units::time;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime};

#[cfg(target_os = "linux")]
use linux_taskstats::{self, Client};

#[cfg(all(feature = "nvidia", target_os = "linux"))]
use nvml::error::NvmlError;
#[cfg(all(feature = "nvidia", target_os = "linux"))]
use nvml::{cuda_driver_version_major, cuda_driver_version_minor};

use std::fs;
use std::path::{Path, PathBuf};
use sysinfo::{
    Component, ComponentExt, Disk, DiskExt, NetworkExt, ProcessExt, ProcessorExt, System, SystemExt,
};
use users::{Users, UsersCache};

#[cfg(all(feature = "nvidia", not(target_os = "linux")))]
#[derive(FromPrimitive, PartialEq, Copy, Clone)]
pub enum ProcessTableSortBy {
    Pid = 0,
    User = 1,
    Priority = 2,
    Nice = 3,
    Cpu = 4,
    MemPerc = 5,
    Mem = 6,
    Virt = 7,
    Status = 8,
    DiskRead = 9,
    DiskWrite = 10,
    Gpu = 11,
    FB = 12,
    Cmd = 13,
}

#[cfg(all(feature = "nvidia", target_os = "linux"))]
#[derive(FromPrimitive, PartialEq, Copy, Clone)]
pub enum ProcessTableSortBy {
    Pid = 0,
    User = 1,
    Priority = 2,
    Nice = 3,
    Cpu = 4,
    MemPerc = 5,
    Mem = 6,
    Virt = 7,
    Status = 8,
    DiskRead = 9,
    DiskWrite = 10,
    IOWait = 11,
    Gpu = 12,
    FB = 13,
    Cmd = 14,
}

#[cfg(all(not(feature = "nvidia"), not(target_os = "linux")))]
#[derive(FromPrimitive, PartialEq, Copy, Clone)]
pub enum ProcessTableSortBy {
    Pid = 0,
    User = 1,
    Priority = 2,
    Nice = 3,
    Cpu = 4,
    MemPerc = 5,
    Mem = 6,
    Virt = 7,
    Status = 8,
    DiskRead = 9,
    DiskWrite = 10,
    Cmd = 11,
}

#[cfg(all(not(feature = "nvidia"), target_os = "linux"))]
#[derive(FromPrimitive, PartialEq, Copy, Clone)]
pub enum ProcessTableSortBy {
    Pid = 0,
    User = 1,
    Priority = 2,
    Nice = 3,
    Cpu = 4,
    MemPerc = 5,
    Mem = 6,
    Virt = 7,
    Status = 8,
    DiskRead = 9,
    DiskWrite = 10,
    IOWait = 11,
    Cmd = 12,
}

#[derive(PartialEq, Eq)]
pub enum ProcessTableSortOrder {
    Ascending = 0,
    Descending = 1,
}

pub trait DiskFreeSpaceExt {
    fn get_perc_free_space(&self) -> f32;
}

impl DiskFreeSpaceExt for Disk {
    fn get_perc_free_space(&self) -> f32 {
        if self.get_total_space() < 1 {
            return 0.0;
        }
        percent_of(self.get_available_space(), self.get_total_space())
    }
}

pub struct NetworkInterface {
    pub name: String,
    pub ip: String,
    pub dest: String,
}

pub struct Sensor {
    pub name: String,
    pub current_temp: f32,
    pub critical: f32,
    pub high: f32,
}

impl From<&Component> for Sensor {
    fn from(c: &Component) -> Sensor {
        Sensor {
            name: c.get_label().to_owned(),
            current_temp: c.get_temperature(),
            critical: c.get_critical().unwrap_or(0.0),
            high: c.get_max(),
        }
    }
}

fn get_max_pid() -> u64 {
    if cfg!(target_os = "macos") {
        99999
    } else if cfg!(target_os = "linux") {
        match fs::read(&Path::new("/proc/sys/kernel/pid_max")) {
            Ok(data) => {
                let r = String::from_utf8_lossy(data.as_slice());
                r.trim().parse::<u64>().unwrap_or(32768)
            }
            Err(_) => 32768,
        }
    } else {
        32768
    }
}

fn get_max_pid_length() -> usize {
    format!("{:}", get_max_pid()).len()
}

pub struct CPUTimeApp {
    pub histogram_map: HistogramMap,
    pub cpu_utilization: u64,
    pub mem_utilization: u64,
    pub mem_total: u64,
    pub swap_utilization: u64,
    pub swap_total: u64,
    pub disks: HashMap<String, ZDisk>,
    pub disk_write: u64,
    pub disk_read: u64,
    pub cpus: Vec<(String, u64)>,
    pub system: System,
    pub net_in: u64,
    pub net_out: u64,
    pub processes: Vec<i32>,
    pub process_map: HashMap<i32, ZProcess>,
    pub user_cache: UsersCache,
    pub cum_cpu_process: Option<ZProcess>,
    pub top_mem_pid: Option<i32>,
    pub top_cpu_pid: Option<i32>,
    pub top_disk_writer_pid: Option<i32>,
    pub top_disk_reader_pid: Option<i32>,
    pub frequency: u64,
    pub threads_total: usize,
    pub psortby: ProcessTableSortBy,
    pub psortorder: ProcessTableSortOrder,
    pub osname: String,
    pub release: String,
    pub version: String,
    pub arch: String,
    pub hostname: String,
    pub network_interfaces: Vec<NetworkInterface>,
    pub sensors: Vec<Sensor>,
    pub gfx_devices: Vec<GraphicsDevice>,
    pub processor_name: String,
    pub started: chrono::DateTime<chrono::Local>,
    pub selected_process: Option<Box<ZProcess>>,
    pub max_pid_len: usize,
    pub batteries: Vec<starship_battery::Battery>,
    pub uptime: Duration,
    #[cfg(all(target_os = "linux", feature = "nvidia"))]
    pub nvml: Option<nvml::NVML>,
    #[cfg(all(target_os = "linux", feature = "nvidia"))]
    pub nvml_error: Option<NvmlError>,
    #[cfg(all(target_os = "linux", feature = "nvidia"))]
    pub nvml_driver_version: Option<String>,
    #[cfg(all(target_os = "linux", feature = "nvidia"))]
    pub nvml_version: Option<String>,
    #[cfg(all(target_os = "linux", feature = "nvidia"))]
    pub nvml_cuda_version: Option<String>,
    #[cfg(target_os = "linux")]
    pub netlink_client: Option<Client>,
}

impl CPUTimeApp {
    pub fn new(tick: Duration, db: Option<PathBuf>) -> CPUTimeApp {
        debug!("Create Histogram Map");
        let histogram_map = HistogramMap::new(Duration::from_secs(60 * 60 * 24), tick, db);
        #[cfg(all(target_os = "linux", feature = "nvidia"))]
        let mut ne = None;
        #[cfg(all(target_os = "linux", feature = "nvidia"))]
        let mut nvml_cuda_version = None;
        #[cfg(all(target_os = "linux", feature = "nvidia"))]
        let mut nvml_version = None;
        #[cfg(all(target_os = "linux", feature = "nvidia"))]
        let mut nvml_driver_version = None;
        #[cfg(all(target_os = "linux", feature = "nvidia"))]
        let nvml = match nvml::NVML::init() {
            Ok(n) => {
                nvml_driver_version = match n.sys_driver_version() {
                    Ok(v) => Some(v),
                    Err(_) => None,
                };
                nvml_version = match n.sys_nvml_version() {
                    Ok(v) => Some(v),
                    Err(_) => None,
                };
                nvml_cuda_version = match n.sys_cuda_driver_version() {
                    Ok(v) => Some(format!(
                        "{:}.{:}",
                        cuda_driver_version_major(v),
                        cuda_driver_version_minor(v)
                    )),
                    Err(_) => None,
                };
                Some(n)
            }
            Err(e) => {
                ne = Some(e);
                None
            }
        };
        let mut s = CPUTimeApp {
            histogram_map,
            cpus: vec![],
            system: System::new_all(),
            cpu_utilization: 0,
            mem_utilization: 0,
            mem_total: 0,
            swap_total: 0,
            swap_utilization: 0,
            disks: HashMap::with_capacity(10),
            net_in: 0,
            net_out: 0,
            processes: Vec::with_capacity(400),
            process_map: HashMap::with_capacity(400),
            user_cache: UsersCache::new(),
            cum_cpu_process: None,
            frequency: 0,
            threads_total: 0,
            disk_read: 0,
            disk_write: 0,
            psortby: ProcessTableSortBy::Cpu,
            psortorder: ProcessTableSortOrder::Descending,
            osname: String::from(""),
            release: String::from(""),
            version: String::from(""),
            arch: String::from(""),
            hostname: String::from(""),
            network_interfaces: vec![],
            sensors: vec![],
            processor_name: String::from(""),
            started: chrono::Local::now(),
            selected_process: None,
            max_pid_len: get_max_pid_length(),
            batteries: vec![],
            top_mem_pid: None,
            top_cpu_pid: None,
            top_disk_reader_pid: None,
            top_disk_writer_pid: None,
            uptime: Duration::from_secs(0),
            gfx_devices: vec![],

            #[cfg(all(target_os = "linux", feature = "nvidia"))]
            nvml: nvml,
            #[cfg(all(target_os = "linux", feature = "nvidia"))]
            nvml_error: ne,
            #[cfg(all(target_os = "linux", feature = "nvidia"))]
            nvml_driver_version: nvml_driver_version,
            #[cfg(all(target_os = "linux", feature = "nvidia"))]
            nvml_version: nvml_version,
            #[cfg(all(target_os = "linux", feature = "nvidia"))]
            nvml_cuda_version: nvml_cuda_version,
            #[cfg(target_os = "linux")]
            netlink_client: match Client::open() {
                Ok(c) => Some(c),
                Err(_) => {
                    debug!("Couldn't open netlink client.");
                    None
                }
            },
        };
        debug!("Initial Metrics Update");
        s.system.refresh_all();
        s.system.refresh_all(); // apparently multiple refreshes are necessary to fill in all values.
        s
    }

    async fn get_platform(&mut self) {
        debug!("Updating Platform");
        match host::platform().await {
            Ok(p) => {
                self.osname = p.system().to_owned();
                self.arch = p.architecture().as_str().to_owned();
                self.hostname = p.hostname().to_owned();
                self.version = p.version().to_owned();
                self.release = p.release().to_owned();
            }
            Err(_) => {
                self.osname = String::from("unknown");
                self.arch = String::from("unknown");
                self.hostname = String::from("unknown");
                self.version = String::from("unknown");
                self.release = String::from("unknown");
            }
        };
    }

    async fn get_uptime(&mut self) {
        if let Ok(u) = host::uptime().await {
            self.uptime = Duration::from_secs_f64(u.get::<time::second>());
        }
    }

    fn get_batteries(&mut self) {
        debug!("Updating Batteries.");
        let manager = starship_battery::Manager::new().expect("Couldn't create battery manager");
        self.batteries = manager
            .batteries()
            .expect("Couldn't get batteries")
            .filter_map(|res| res.ok())
            .collect();
    }

    async fn get_nics(&mut self) {
        debug!("Updating Network Interfaces");
        self.network_interfaces.clear();
        let nics = net::nic().await;
        let nics = match nics {
            Ok(nics) => nics,
            Err(_) => {
                debug!("Couldn't get nic information");
                return;
            }
        };
        ::futures::pin_mut!(nics);
        while let Some(n) = nics.next().await {
            match n {
                Ok(n) => {
                    if !n.is_up() || n.is_loopback() {
                        continue;
                    }
                    if n.name().starts_with("utun")
                        || n.name().starts_with("awd")
                        || n.name().starts_with("ham")
                    {
                        continue;
                    }
                    let ip = match n.address() {
                        Address::Inet(n) => n.to_string(),
                        _ => String::new(),
                    }
                    .trim_end_matches(":0")
                    .to_string();
                    if ip.is_empty() {
                        continue;
                    }
                    let dest = match n.destination() {
                        Some(Address::Inet(d)) => d.to_string(),
                        _ => String::new(),
                    };
                    self.network_interfaces.push(NetworkInterface {
                        name: n.name().to_owned(),
                        ip,
                        dest,
                    });
                }
                Err(_) => println!("Couldn't get information on a nic"),
            }
        }
    }

    async fn update_sensors(&mut self) {
        self.sensors.clear();
        for t in self.system.get_components() {
            if cfg!(target_os = "linux") {
                if t.get_label().contains("Package id") {
                    debug!("{:?}", t);
                    self.sensors.push(Sensor::from(t));
                }
            } else if cfg!(target_os = "macos") && t.get_label().contains("CPU") {
                self.sensors.push(Sensor::from(t));
            }
        }
    }

    pub fn select_process(&mut self, highlighted_process: Option<Box<ZProcess>>) {
        debug!("Selected Process.");
        self.selected_process = highlighted_process;
    }

    fn update_process_list(&mut self, keep_order: bool) {
        debug!("Updating Process List");
        let process_list = self.system.get_processes();
        #[cfg(target_os = "linux")]
        let client = &self.netlink_client;
        let mut current_pids: HashSet<i32> = HashSet::with_capacity(process_list.len());

        #[derive(Default)]
        struct ValAndPid<T> {
            val: T,
            pid: Option<i32>,
        }
        impl<T: PartialOrd> ValAndPid<T> {
            fn update(&mut self, new: T, pid: i32) {
                if new > self.val {
                    self.val = new;
                    self.pid = Some(pid);
                }
            }
        }

        #[derive(Default)]
        struct Top {
            cum_cpu: ValAndPid<f64>,
            cpu: ValAndPid<f32>,
            mem: ValAndPid<u64>,
            read: ValAndPid<f64>,
            write: ValAndPid<f64>,
        }
        impl Top {
            fn update(&mut self, zp: &ZProcess, tick_rate: &Duration) {
                self.cum_cpu.update(zp.cum_cpu_usage, zp.pid);
                self.cpu.update(zp.cpu_usage, zp.pid);
                self.mem.update(zp.memory, zp.pid);
                self.read.update(zp.get_read_bytes_sec(tick_rate), zp.pid);
                self.write.update(zp.get_write_bytes_sec(tick_rate), zp.pid);
            }
        }
        let mut top = Top::default();
        top.cum_cpu.val = match &self.cum_cpu_process {
            Some(p) => p.cum_cpu_usage,
            None => 0.0,
        };

        self.threads_total = 0;

        for (pid, process) in process_list {
            if let Some(zp) = self.process_map.get_mut(pid) {
                if zp.start_time == process.start_time() {
                    let disk_usage = process.disk_usage();
                    // check for PID reuse
                    zp.memory = process.memory();
                    zp.cpu_usage = process.cpu_usage();
                    zp.cum_cpu_usage += zp.cpu_usage as f64;
                    zp.status = process.status();
                    zp.priority = process.priority;
                    zp.nice = process.nice;
                    zp.virtual_memory = process.virtual_memory();
                    zp.threads_total = process.threads_total;
                    self.threads_total += zp.threads_total as usize;
                    zp.prev_read_bytes = zp.read_bytes;
                    zp.prev_write_bytes = zp.write_bytes;
                    zp.read_bytes = disk_usage.total_read_bytes;
                    zp.write_bytes = disk_usage.total_written_bytes;
                    zp.last_updated = SystemTime::now();
                    #[cfg(target_os = "linux")]
                    zp.update_delay(client);

                    top.update(zp, &self.histogram_map.tick);
                } else {
                    let user_name = self
                        .user_cache
                        .get_user_by_uid(process.uid)
                        .map(|user| user.name().to_string_lossy().to_string())
                        .unwrap_or_default();
                    let zprocess = ZProcess::from_user_and_process(user_name, process);
                    self.threads_total += zprocess.threads_total as usize;

                    top.update(zp, &self.histogram_map.tick);

                    self.process_map.insert(zprocess.pid, zprocess);
                }
            } else {
                let user_name = self
                    .user_cache
                    .get_user_by_uid(process.uid)
                    .map(|user| user.name().to_string_lossy().to_string())
                    .unwrap_or_default();
                #[allow(unused_mut)]
                let mut zprocess = ZProcess::from_user_and_process(user_name, process);
                #[cfg(target_os = "linux")]
                zprocess.update_delay(client);

                self.threads_total += zprocess.threads_total as usize;

                top.update(&zprocess, &self.histogram_map.tick);

                self.process_map.insert(zprocess.pid, zprocess);
            }
            current_pids.insert(*pid);
        }

        if keep_order {
            self.processes.retain(|pid| current_pids.contains(pid));
        } else {
            self.processes = current_pids.iter().cloned().collect();
        }

        // remove pids that are gone
        self.process_map.retain(|&k, _| current_pids.contains(&k));

        //set top cumulative process if we've changed it.
        if let Some(p) = top.cum_cpu.pid {
            if let Some(p) = self.process_map.get(&p) {
                self.cum_cpu_process = Some(p.clone())
            }
        } else if let Some(p) = &mut self.cum_cpu_process {
            if let Some(cp) = self.process_map.get(&p.pid) {
                if cp.start_time == p.start_time {
                    self.cum_cpu_process = Some(cp.clone());
                } else {
                    p.set_end_time();
                }
            } else {
                // our cumulative winner is dead
                p.set_end_time();
            }
        }

        // update top mem / disk reader & writer
        self.top_mem_pid = top.mem.pid.or(self.top_mem_pid);
        self.top_disk_reader_pid = top.read.pid.or(self.top_disk_reader_pid);
        self.top_disk_writer_pid = top.write.pid.or(self.top_disk_writer_pid);
        self.top_cpu_pid = top.cpu.pid.or(self.top_cpu_pid);

        // update selected process
        if let Some(p) = self.selected_process.as_mut() {
            let pid = &p.pid;
            if let Some(proc) = self.process_map.get(pid) {
                self.selected_process = Some(Box::new(proc.clone()));
            } else {
                p.set_end_time();
            }
        }

        if !keep_order {
            self.sort_process_table();
        }
    }

    pub fn sort_process_table(&mut self) {
        debug!("Sorting Process Table");
        let pm = &self.process_map;
        let sorter = ZProcess::field_comparator(self.psortby);
        let sortorder = &self.psortorder;
        let tick = self.histogram_map.tick;
        self.processes.sort_by(|a, b| {
            let pa = pm.get(a).expect("Error in sorting the process table.");
            let pb = pm.get(b).expect("Error in sorting the process table.");

            let ord = sorter(pa, pb, &tick);
            match sortorder {
                ProcessTableSortOrder::Ascending => ord,
                ProcessTableSortOrder::Descending => ord.reverse(),
            }
        });
    }

    async fn update_frequency(&mut self) {
        debug!("Updating Frequency");
        let f = heim::cpu::frequency().await;
        if let Ok(f) = f {
            self.frequency = f.current().get::<megahertz>();
        }
    }

    async fn update_disk(&mut self) {
        debug!("Updating Disks");

        static IGNORED_FILE_SYSTEMS: &[&[u8]] = &[
            b"sysfs",
            b"proc",
            b"tmpfs",
            b"cgroup",
            b"cgroup2",
            b"pstore",
            b"squashfs",
            b"iso9660",
        ];

        self.system.refresh_disks_list();
        let mut updated: HashMap<String, bool> = HashMap::with_capacity(self.disks.len());
        for k in self.disks.keys() {
            if k == "Total" {
                continue;
            }
            updated.insert(k.to_string(), false);
        }

        let mut total_available = 0;
        let mut total_space = 0;

        for d in self.system.get_disks().iter() {
            let name = d.get_name().to_string_lossy();
            let mp = d.get_mount_point().to_string_lossy();
            if cfg!(target_os = "linux") {
                let fs = d.get_file_system();
                if IGNORED_FILE_SYSTEMS.iter().any(|ignored| &fs == ignored) {
                    continue;
                }
                if mp.starts_with("/sys")
                    || mp.starts_with("/proc")
                    || mp.starts_with("/run")
                    || mp.starts_with("/dev")
                    || name.starts_with("shm")
                    || name.starts_with("sunrpc")
                {
                    continue;
                }
            }
            let name = get_device_name(d.get_name());
            let mut zd = self.disks.entry(name).or_insert(ZDisk::from_disk(&d));
            zd.size_bytes = d.get_total_space();
            zd.available_bytes = d.get_available_space();
            total_available += zd.available_bytes;
            total_space += zd.size_bytes;
            updated.insert(zd.name.to_string(), true);
            self.histogram_map.add_value_to(
                &HistogramKind::FileSystemUsedSpace(zd.name.to_string()),
                zd.get_used_bytes(),
            );
        }
        for (k, v) in updated.iter() {
            if !v {
                self.disks.remove(k);
            }
        }

        self.disk_read = self
            .process_map
            .iter()
            .map(|(_pid, p)| p.get_read_bytes_sec(&self.histogram_map.tick) as u64)
            .sum();
        self.disk_write = self
            .process_map
            .iter()
            .map(|(_pid, p)| p.get_write_bytes_sec(&self.histogram_map.tick) as u64)
            .sum();

        get_disk_io_metrics(&mut self.disks).await;

        let mut previous_io = IoMetrics { read_bytes: 0, write_bytes: 0};
        let mut current_io = IoMetrics { read_bytes: 0, write_bytes: 0};

        #[cfg(target_os = "linux")]
        for d in self.disks.values(){
            if d.mount_point.to_string_lossy() != "Total"{
                previous_io += d.previous_io;
                current_io += d.current_io;
            }
        }
        #[cfg(not(target_os = "linux"))]
        for p in self.process_map.values(){
            previous_io.read_bytes += p.prev_read_bytes;
            previous_io.write_bytes += p.prev_write_bytes;
            current_io.read_bytes += p.read_bytes;
            current_io.write_bytes += p.write_bytes;
        }

        self.update_disk_histograms(total_available, total_space, previous_io, current_io).await;
        
    }

    pub async fn update_disk_histograms(&mut self,total_available: u64, total_space: u64, previous_io: IoMetrics, current_io: IoMetrics){
        let mut overall = self.disks.entry("Total".to_string())
                                               .or_insert(ZDisk::new_total());
        overall.available_bytes = total_available;
        overall.size_bytes = total_space;
        overall.previous_io = previous_io;
        overall.current_io = current_io;
        self.histogram_map.add_value_to(
            &HistogramKind::FileSystemUsedSpace(overall.name.to_string()),
            overall.get_used_bytes(),
        );        

        for disk in self.disks.values(){
            self.histogram_map
            .add_value_to(&HistogramKind::IoRead(disk.name.to_string()), disk.get_read_bytes_sec(&self.histogram_map.tick) as u64);
        self.histogram_map
            .add_value_to(&HistogramKind::IoWrite(disk.name.to_string()), disk.get_write_bytes_sec(&self.histogram_map.tick) as u64);
        }
    }

    pub async fn update_cpu(&mut self) {
        debug!("Updating CPU");
        let procs = self.system.get_processors();
        let mut usage: f32 = 0.0;
        self.cpus.clear();
        let mut usagev: Vec<f32> = vec![];
        for (i, p) in procs.iter().enumerate() {
            if i == 0 {
                self.processor_name = p.get_name().to_owned();
            }
            let mut u = p.get_cpu_usage();
            if u.is_nan() {
                u = 0.0;
            }
            self.cpus.push((format!("{}", i + 1), u as u64));
            usage += u;
            usagev.push(u);
        }
        if procs.is_empty() {
            self.cpu_utilization = 0;
        } else {
            usage /= procs.len() as f32;
            self.cpu_utilization = usage as u64;
        }
        self.histogram_map
            .add_value_to(&HistogramKind::Cpu, self.cpu_utilization);
    }

    pub async fn update_networks(&mut self) {
        let mut net_in = 0;
        let mut net_out = 0;
        for (_iface, data) in self.system.get_networks() {
            debug!("iface: {}", _iface);
            net_in += data.get_received();
            net_out += data.get_transmitted();
        }
        self.net_in = net_in;
        self.net_out = net_out;
        self.histogram_map
            .add_value_to(&HistogramKind::NetRx, self.net_in);
        self.histogram_map
            .add_value_to(&HistogramKind::NetTx, self.net_out);
    }

    pub async fn update(&mut self, keep_order: bool) {
        debug!("Updating Metrics");
        self.system.refresh_all();
        self.update_cpu().await;
        self.update_sensors().await;

        self.mem_utilization = self.system.get_used_memory();
        self.mem_total = self.system.get_total_memory();

        let mem = percent_of(self.mem_utilization, self.mem_total) as u64;

        self.histogram_map.add_value_to(&HistogramKind::Mem, mem);

        self.swap_utilization = self.system.get_used_swap();
        self.swap_total = self.system.get_total_swap();

        self.update_networks().await;
        self.update_process_list(keep_order);
        self.update_frequency().await;
        self.update_disk().await;
        self.get_platform().await;
        self.get_nics().await;
        self.get_batteries();
        self.get_uptime().await;
        self.update_gfx_devices();
        self.update_gpu_utilization();
        debug!("Updated Metrics for {} processes.", self.processes.len());
    }

    pub async fn save_state(&mut self) {
        self.histogram_map.save_histograms();
    }

    pub fn writes_db_store(&self) -> bool {
        self.histogram_map.writes_db_store()
    }
}
