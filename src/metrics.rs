/**
 * Copyright 2019 Benjamin Vaisvil
 */
use crate::util::percent_of;
use crate::zprocess::*;

use futures::StreamExt;
use heim::host;
use heim::net;
use heim::net::Address;
use heim::units::frequency::megahertz;
use heim::units::time;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::time::{Duration, SystemTime};

#[cfg(all(target_os = "linux", feature = "nvidia"))]
use nvml::enum_wrappers::device::{Clock, TemperatureSensor, TemperatureThreshold};
#[cfg(all(target_os = "linux", feature = "nvidia"))]
use nvml::struct_wrappers::device::ProcessUtilizationSample;
#[cfg(all(target_os = "linux", feature = "nvidia"))]
use nvml::NVML;
use serde_derive::{Deserialize, Serialize};

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use sysinfo::{Disk, DiskExt, NetworkExt, NetworksExt, Process, ProcessExt, ProcessorExt, System, SystemExt};
use users::{Users, UsersCache};

const ONE_WEEK: u64 = 60 * 60 * 24 * 7;
const DB_ERROR: &str = "Couldn't open database.";
const DSER_ERROR: &str = "Couldn't deserialize object";
const SER_ERROR: &str = "Couldn't serialize object";

#[cfg(feature = "nvidia")]
#[derive(FromPrimitive, PartialEq, Copy, Clone)]
pub enum ProcessTableSortBy {
    Pid = 0,
    User = 1,
    Priority = 2,
    Nice = 3,
    CPU = 4,
    MemPerc = 5,
    Mem = 6,
    Virt = 7,
    Status = 8,
    DiskRead = 9,
    DiskWrite = 10,
    GPU = 11,
    FB = 12,
    Cmd = 13,
}

#[cfg(not(feature = "nvidia"))]
#[derive(FromPrimitive, PartialEq, Copy, Clone)]
pub enum ProcessTableSortBy {
    Pid = 0,
    User = 1,
    Priority = 2,
    Nice = 3,
    CPU = 4,
    MemPerc = 5,
    Mem = 6,
    Virt = 7,
    Status = 8,
    DiskRead = 9,
    DiskWrite = 10,
    Cmd = 11,
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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Histogram {
    pub data: Vec<u64>,
}

impl Histogram {
    fn new(size: usize) -> Histogram {
        Histogram {
            data: vec![0; size],
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HistogramMap {
    map: HashMap<String, Histogram>,
    duration: Duration,
    pub tick: Duration,
    db: Option<PathBuf>,
    previous_stop: Option<SystemTime>,
}

fn load_zenith_store(path: PathBuf, current_time: &SystemTime) -> HistogramMap {
    // need to fill in time between when it was last stored and now, like the sled DB
    let data = std::fs::read(path).expect(DB_ERROR);
    let mut hm: HistogramMap = bincode::deserialize(&data).expect(DSER_ERROR);
    match hm.previous_stop {
        Some(previous_stop) => {
            if previous_stop < *current_time {
                let d = current_time
                    .duration_since(previous_stop)
                    .expect("Current time is before stored time. This should not happen.");
                let week_ticks = ONE_WEEK / hm.tick.as_secs();
                for (_k, v) in hm.map.iter_mut() {
                    if v.data.len() as u64 > week_ticks {
                        let end = v.data.len() as u64 - week_ticks;
                        v.data.drain(0..end as usize);
                    }
                    let mut dur = d;
                    // add 0s between then and now.
                    let zero_dur = Duration::from_secs(0);
                    while dur > zero_dur + hm.tick {
                        v.data.push(0);
                        dur -= hm.tick;
                    }
                }
            }
            hm
        }
        None => hm,
    }
}

impl HistogramMap {
    fn new(dur: Duration, tick: Duration, db: Option<PathBuf>) -> HistogramMap {
        let current_time = SystemTime::now();
        let path = match &db {
            Some(db) => Some(db.to_owned()),
            None => None,
        };
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

    fn add(&mut self, name: &str) -> &mut Histogram {
        let size = (self.duration.as_secs() / self.tick.as_secs()) as usize; //smallest has to be >= 1000ms
        let names = name.to_owned();
        let _r = self.map.insert(names, Histogram::new(size));
        self.get_mut(name)
            .expect("Unexpectedly couldn't get mutable reference to value we just added.")
    }

    pub fn get_zoomed(
        &self,
        name: &str,
        zoom_factor: u32,
        update_number: u32,
        width: usize,
        offset: usize,
    ) -> Option<Histogram> {
        match self.get(name) {
            Some(h) => {
                let mut nh = Histogram::new(width);
                let mut h = h.clone();
                for _i in 0..zoom_factor as usize * offset {
                    h.data.pop();
                }
                let nh_len = nh.data.len();
                let zf = zoom_factor as usize;
                let mut si: usize = if (width * zf) > h.data.len() {
                    0
                } else {
                    h.data.len() - (width * zf) - update_number as usize
                };

                for index in 0..nh_len {
                    if si + zf <= h.data.len() {
                        nh.data[index] = h.data[si..si + zf].iter().sum::<u64>();
                    } else {
                        nh.data[index] = h.data[si..].iter().sum::<u64>();
                    }
                    si += zf;
                }

                nh.data = nh.data.iter().map(|d| d / zoom_factor as u64).collect();
                Some(nh)
            }
            None => None,
        }
    }

    pub fn get(&self, name: &str) -> Option<&Histogram> {
        self.map.get(name)
    }

    fn get_mut(&mut self, name: &str) -> Option<&mut Histogram> {
        self.map.get_mut(name)
    }

    fn has(&self, name: &str) -> bool {
        self.map.contains_key(name)
    }

    fn add_value_to(&mut self, name: &str, val: u64) {
        let h: &mut Histogram = if self.has(name) {
            self.get_mut(name).expect("Couldn't get mutable reference")
        }
        else{
            self.add(name)
        };
        h.data.push(val);
        debug!("Adding {} to {} chart.", val, name);
    }

    pub fn hist_duration(&self, width: usize, zoom_factor: u32) -> chrono::Duration {
        chrono::Duration::from_std(Duration::from_secs_f64(
            self.tick.as_secs_f64() * width as f64 * zoom_factor as f64,
        ))
        .expect("Unexpectedly large duration was out of range.")
    }

    pub fn histograms_width(&self) -> Option<usize> {
        match self.map.iter().next() {
            Some((_k, h)) => Some(h.data.len()),
            None => None,
        }
    }

    fn save_histograms(&mut self) {
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

#[derive(Clone)]
pub struct GFXDeviceProcess{
    pub pid: i32,
    pub timestamp: u64,
    pub sm_utilization: u32,
    pub mem_utilization: u32,
    pub enc_utilization: u32,
    pub dec_utilization: u32
}

impl GFXDeviceProcess{
    #[cfg(all(target_os = "linux", feature = "nvidia"))]
    fn from_nvml(process: &ProcessUtilizationSample) -> GFXDeviceProcess{
        GFXDeviceProcess{
            pid: process.pid as i32,
            timestamp: process.timestamp,
            sm_utilization: process.sm_util,
            mem_utilization: process.mem_util,
            enc_utilization: process.enc_util,
            dec_utilization: process.dec_util
        }
    }
}

#[derive(Clone)]
pub struct GFXDevice {
    pub name: String,
    pub gpu_utilization: u32,
    pub decoder_utilization: u32,
    pub encoder_utilization: u32,
    pub mem_utilization: u32,
    pub total_memory: u64,
    pub used_memory: u64,
    pub fans: Vec<u32>,
    pub temperature: u32,
    pub temperature_max: u32,
    pub power_usage: u32,
    pub max_power: u32,
    pub clock: u32,
    pub max_clock: u32,
    pub uuid: String,
    pub processes: Vec<GFXDeviceProcess>
}

impl GFXDevice {

    #[allow(dead_code)]
    fn new(uuid: String) -> GFXDevice {
        GFXDevice {
            name: String::from(""),
            gpu_utilization: 0,
            encoder_utilization: 0,
            decoder_utilization: 0,
            mem_utilization: 0,
            total_memory: 0,
            used_memory: 0,
            fans: vec![],
            temperature: 0,
            temperature_max: 0,
            power_usage: 0,
            max_power: 0,
            clock: 0,
            max_clock: 0,
            uuid,
            processes: vec![]
        }
    }

    #[cfg(all(target_os = "linux", feature = "nvidia"))]
    fn processes_from_nvml(&mut self, processes: Vec<ProcessUtilizationSample>){
        self.processes = processes.iter().map(|p| GFXDeviceProcess::from_nvml(p)).collect()
    }
}

impl fmt::Display for GFXDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: GPU: {}% MEM: {}%",
            self.name, self.gpu_utilization, self.mem_utilization
        )
    }
}

pub struct ZDisk{
    pub mount_point: PathBuf,
    pub available_bytes: u64,
    pub size_bytes: u64
}

impl ZDisk{
    fn from_disk(d: &Disk) -> ZDisk{
        ZDisk{
            mount_point: d.get_mount_point().to_path_buf(),
            available_bytes: d.get_available_space(),
            size_bytes: d.get_total_space()
        }
    }

    pub fn get_perc_free_space(&self) -> f32{
        if self.size_bytes < 1 {
            return 0.0;
        }
        percent_of(self.available_bytes, self.size_bytes)
    }
}

pub struct CPUTimeApp {
    pub histogram_map: HistogramMap,
    pub cpu_utilization: u64,
    pub mem_utilization: u64,
    pub mem_total: u64,
    pub swap_utilization: u64,
    pub swap_total: u64,
    pub disks: Vec<ZDisk>,
    pub disk_total: u64,
    pub disk_available: u64,
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
    pub gfx_devices: Vec<GFXDevice>,
    pub processor_name: String,
    pub started: chrono::DateTime<chrono::Local>,
    pub selected_process: Option<ZProcess>,
    pub max_pid_len: usize,
    pub batteries: Vec<battery::Battery>,
    pub uptime: Duration,
}

impl CPUTimeApp {
    pub fn new(tick: Duration, db: Option<PathBuf>) -> CPUTimeApp {
        debug!("Create Histogram Map");
        let histogram_map = HistogramMap::new(Duration::from_secs(60 * 60 * 24), tick, db);
        let mut s = CPUTimeApp {
            histogram_map,
            cpus: vec![],
            system: System::new_all(),
            cpu_utilization: 0,
            mem_utilization: 0,
            mem_total: 0,
            swap_total: 0,
            swap_utilization: 0,
            disks: vec![],
            disk_available: 0,
            disk_total: 0,
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
            psortby: ProcessTableSortBy::CPU,
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
            top_disk_reader_pid: None,
            top_disk_writer_pid: None,
            uptime: Duration::from_secs(0),
            gfx_devices: vec![],
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
        let manager = battery::Manager::new().expect("Couldn't create battery manager");
        self.batteries.clear();
        for b in manager.batteries().expect("Couldn't get batteries") {
            if let Ok(b) = b {
                self.batteries.push(b)
            }
        }
    }

    async fn get_nics(&mut self) {
        debug!("Updating Network Interfaces");
        self.network_interfaces.clear();
        let nics = net::nic().await;
        match nics {
            Ok(nics) => {
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
                                _ => format!(""),
                            }
                            .trim_end_matches(":0")
                            .to_string();
                            if ip.is_empty() {
                                continue;
                            }
                            let dest = match n.destination() {
                                Some(d) => match d {
                                    Address::Inet(d) => d.to_string(),
                                    _ => format!(""),
                                },
                                None => format!(""),
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
            Err(_) => debug!("Couldn't get nic information"),
        }
    }

    #[cfg(not(all(target_os = "linux", feature = "nvidia")))]
    async fn update_gfx_devices(&mut self) {}

    #[cfg(all(target_os = "linux", feature = "nvidia"))]
    async fn update_gfx_devices(&mut self) {
        self.gfx_devices.clear();
        let nvml = NVML::init();
        match nvml {
            Ok(n) => {
                let count = n.device_count().unwrap_or(0);
                for i in 0..count {
                    match n.device_by_index(i) {
                        Ok(d) => match d.uuid() {
                            Ok(uuid) => {
                                let mut gd = GFXDevice::new(uuid);
                                match d.memory_info() {
                                    Ok(m) => {
                                        gd.total_memory = m.total;
                                        gd.used_memory = m.used;
                                    }
                                    Err(e) => error!("Failed Getting Memory Info {:?}", e),
                                }
                                gd.name = d.name().unwrap_or(String::from(""));
                                gd.clock = d.clock_info(Clock::Graphics).unwrap_or(0);
                                gd.max_clock = d.max_clock_info(Clock::Graphics).unwrap_or(0);
                                gd.power_usage = d.power_usage().unwrap_or(0);
                                gd.max_power = d.power_management_limit().unwrap_or(0);
                                gd.temperature = d.temperature(TemperatureSensor::Gpu).unwrap_or(0);
                                gd.temperature_max = d
                                    .temperature_threshold(TemperatureThreshold::GpuMax)
                                    .unwrap_or(0);
                                for i in 0..4 {
                                    let r = d.fan_speed(i);
                                    if r.is_ok() {
                                        gd.fans.push(r.unwrap_or(0));
                                    } else {
                                        break;
                                    }
                                }
                                gd.decoder_utilization = match d.decoder_utilization(){
                                    Ok(u) => u.utilization,
                                    Err(_) => 0
                                };
                                gd.encoder_utilization = match d.encoder_utilization(){
                                    Ok(u) => u.utilization,
                                    Err(_) => 0
                                };
                                match d.utilization_rates() {
                                    Ok(u) => {
                                        self.histogram_map.add_value_to(
                                            format!("{}_gpu", gd.uuid).as_str(),
                                            u.gpu as u64,
                                        );
                                        self.histogram_map.add_value_to(
                                            format!("{}_mem", gd.uuid).as_str(),
                                            u.memory as u64,
                                        );
                                        gd.gpu_utilization = u.gpu;
                                        gd.mem_utilization = u.memory;
                                    }
                                    Err(e) => error!("Couldn't get utilization rates: {:?}", e),
                                }
                                match d.process_utilization_stats(None){
                                    Ok(ps) => gd.processes_from_nvml(ps),
                                    Err(_) => { debug!("Couldn't retrieve process utilization stats for {:}", gd.name)}
                                }
                                debug!("{:}", gd);
                                // mock device code to test multiple cards.
                                //let mut gd2 = gd.clone();
                                self.gfx_devices.push(gd);
                                //
                                 //gd2.name = String::from("Card2");
                                 //gd2.max_clock = 1000;
                                 //self.gfx_devices.push(gd2);
                            }
                            Err(e) => error!("Couldn't get UUID: {}", e),
                        },
                        Err(e) => error!("Couldn't get gfx device at index: {}: {:?}", i, e),
                    }
                }
            }
            Err(e) => error!("Couldn't init NVML: {:?}", e),
        }
    }

    //    async fn update_sensors(&mut self){
    //        let mut sensors = sensors::temperatures();
    //        self.sensors.clear();
    //        while let Some(s) = sensors.next().await{
    //            let s = s.unwrap();
    //            self.sensors.push(Sensor{name: s.label().unwrap_or("").to_owned(),
    //             current_temp: s.current().value, critical: 0.0, high: 0.0})
    //        }
    //    }

    // async fn update_sensors(&mut self) {
    //     self.sensors.clear();
    //     for t in self.system.get_components_list() {
    //         if t.get_temperature() < 1.0{
    //             continue;
    //         }
    //         self.sensors.push(Sensor {
    //             name: t.get_label().to_owned(),
    //             current_temp: t.get_temperature(),
    //             high: t.get_max(),
    //             critical: t.get_critical().unwrap_or(0.0),
    //         });
    //         self.histogram_map.add_value_to(t.get_label().to_owned().as_str(), t.get_temperature() as u64);
    //     }
    // }

    // pub fn highlight_up(&mut self) {
    //     if self.highlighted_row != 0 {
    //         self.highlighted_row -= 1;
    //     }
    // }

    // pub fn highlight_down(&mut self) {
    //     if self.highlighted_row < self.process_map.len() {
    //         self.highlighted_row += 1;
    //     }
    // }

    pub fn select_process(&mut self, highlighted_process: Option<ZProcess>) {
        debug!("Selected Process.");
        self.selected_process = highlighted_process;
    }

    fn copy_to_zprocess(&self, process: &Process) -> ZProcess {
        let user_name = match self.user_cache.get_user_by_uid(process.uid) {
            Some(user) => user.name().to_string_lossy().to_string(),
            None => String::from(""),
        };
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
            sm_utilization: 0
        }
    }

    fn update_process_list(&mut self, keep_order: bool) {
        debug!("Updating Process List");
        let process_list = self.system.get_processes();
        let mut current_pids: HashSet<i32> = HashSet::with_capacity(process_list.len());
        let mut top_pid: Option<i32> = None;
        let mut top_cum_cpu_usage: f64 = match &self.cum_cpu_process {
            Some(p) => p.cum_cpu_usage,
            None => 0.0,
        };
        let mut top_mem = 0;
        let mut top_mem_pid = 0;
        let mut top_read = 0.0;
        let mut top_reader = 0;
        let mut top_write = 0.0;
        let mut top_writer = 0;
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
                    if zp.cum_cpu_usage > top_cum_cpu_usage {
                        top_pid = Some(zp.pid);
                        top_cum_cpu_usage = zp.cum_cpu_usage;
                    }
                    if zp.memory > top_mem {
                        top_mem = zp.memory;
                        top_mem_pid = zp.pid;
                    }
                    if zp.get_read_bytes_sec() > top_read {
                        top_read = zp.get_read_bytes_sec();
                        top_reader = zp.pid;
                    }
                    if zp.get_write_bytes_sec() > top_write {
                        top_write = zp.get_write_bytes_sec();
                        top_writer = zp.pid;
                    }
                } else {
                    let zprocess = self.copy_to_zprocess(&process);
                    self.threads_total += zprocess.threads_total as usize;
                    if zprocess.cum_cpu_usage > top_cum_cpu_usage {
                        top_pid = Some(zprocess.pid);
                        top_cum_cpu_usage = zprocess.cum_cpu_usage;
                    }
                    if zprocess.memory > top_mem {
                        top_mem = zprocess.memory;
                        top_mem_pid = zprocess.pid;
                    }
                    if zprocess.get_read_bytes_sec() > top_read {
                        top_read = zprocess.get_read_bytes_sec();
                        top_reader = zprocess.pid;
                    }
                    if zprocess.get_read_bytes_sec() > top_write {
                        top_write = zprocess.get_read_bytes_sec();
                        top_writer = zprocess.pid;
                    }
                    self.process_map.insert(zprocess.pid, zprocess);
                }
            } else {
                let zprocess = self.copy_to_zprocess(&process);
                self.threads_total += zprocess.threads_total as usize;
                if zprocess.cum_cpu_usage > top_cum_cpu_usage {
                    top_pid = Some(zprocess.pid);
                    top_cum_cpu_usage = zprocess.cum_cpu_usage;
                }
                if zprocess.memory > top_mem {
                    top_mem = zprocess.memory;
                    top_mem_pid = zprocess.pid;
                }
                if zprocess.get_read_bytes_sec() > top_read {
                    top_read = zprocess.get_read_bytes_sec();
                    top_reader = zprocess.pid;
                }
                if zprocess.get_write_bytes_sec() > top_write {
                    top_write = zprocess.get_write_bytes_sec();
                    top_writer = zprocess.pid;
                }
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
        if let Some(p) = top_pid {
            if let Some(p) = self.process_map.get(&p) {
                self.cum_cpu_process = Some(p.clone())
            }
        } else if let Some(p) = &mut self.cum_cpu_process {
            if self.process_map.contains_key(&p.pid) {
                if let Some(cp) = self.process_map.get(&p.pid) {
                    if cp.start_time == p.start_time {
                        self.cum_cpu_process = Some(cp.clone());
                    } else {
                        p.set_end_time();
                    }
                }
            } else {
                // our cumulative winner is dead
                p.set_end_time();
            }
        }

        // update top mem / disk reader & writer
        if top_mem_pid > 0 {
            self.top_mem_pid = Some(top_mem_pid);
        }
        if top_reader > 0 {
            self.top_disk_reader_pid = Some(top_reader);
        }
        if top_writer > 0 {
            self.top_disk_writer_pid = Some(top_writer);
        }

        // update selected process
        if let Some(p) = self.selected_process.as_mut() {
            let pid = &p.pid;
            if self.process_map.contains_key(pid) {
                self.selected_process = Some(self.process_map[pid].clone());
            } else {
                p.set_end_time();
            }
        }

        if !keep_order {
            self.sort_process_table();
        }
    }

    //#[cfg(not(feature = "nvidia"))]
    pub fn sort_process_table(&mut self) {
        debug!("Sorting Process Table");
        let pm = &self.process_map;
        let sorter = ZProcess::field_comparator(self.psortby);
        let sortorder = &self.psortorder;
        self.processes.sort_by(|a, b| {
            let pa = pm.get(a).expect("Error in sorting the process table.");
            let pb = pm.get(b).expect("Error in sorting the process table.");

            let ord = sorter(pa, pb);
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

    async fn update_gpu_utilization(&mut self){
        for d in &mut self.gfx_devices{
            for p in &d.processes{
                let proc = self.process_map.get_mut(&p.pid);
                if let Some(proc) = proc {
                    proc.gpu_usage = (p.sm_utilization + p.dec_utilization + p.enc_utilization) as u64;
                    proc.fb_utilization = p.mem_utilization as u64;
                    proc.dec_utilization = p.dec_utilization as u64;
                    proc.enc_utilization = p.enc_utilization as u64;
                    proc.enc_utilization = p.enc_utilization as u64;
                }
            }
        }
    }

    fn update_disk(&mut self, _width: u16) {
        debug!("Updating Disks");
        self.disk_available = 0;
        self.disk_total = 0;
        self.disks.clear();

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
            self.disk_available += d.get_available_space();
            self.disk_total += d.get_total_space();

            self.disks.push(ZDisk::from_disk(d));
        }

        self.disk_read = self
            .process_map
            .iter()
            .map(|(_pid, p)| p.get_read_bytes_sec() as u64)
            .sum();
        self.disk_write = self
            .process_map
            .iter()
            .map(|(_pid, p)| p.get_write_bytes_sec() as u64)
            .sum();

        self.histogram_map.add_value_to("disk_read", self.disk_read);
        self.histogram_map
            .add_value_to("disk_write", self.disk_write);
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
            self.cpus
                .push((format!("{}", i + 1), u as u64));
            usage += u;
            usagev.push(u);
        }
        if procs.len() == 0 {
            self.cpu_utilization = 0;
        } else {
            usage /= procs.len() as f32;
            self.cpu_utilization = usage as u64;
        }
        self.histogram_map
            .add_value_to("cpu_usage_histogram", self.cpu_utilization);
    }

    pub async fn update_networks(&mut self){
        let mut net_in = 0;
        let mut net_out = 0;
        for (_iface, data) in self.system.get_networks(){
            net_in += data.get_received();
            net_out += data.get_transmitted();
        }
        self.net_in = net_in;
        self.net_out = net_out;
        self.histogram_map.add_value_to("net_in", self.net_in);
        self.histogram_map.add_value_to("net_out", self.net_out);
    }

    pub async fn update(&mut self, width: u16, keep_order: bool) {
        debug!("Updating Metrics");
        self.system.refresh_all();
        self.update_cpu().await;
        //self.update_sensors().await;

        self.mem_utilization = self.system.get_used_memory();
        self.mem_total = self.system.get_total_memory();

        let mem = percent_of(self.mem_utilization, self.mem_total) as u64;

        self.histogram_map.add_value_to("mem_utilization", mem);

        self.swap_utilization = self.system.get_used_swap();
        self.swap_total = self.system.get_total_swap();

        self.update_networks().await;
        self.update_process_list(keep_order);
        self.update_frequency().await;
        self.update_disk(width);
        self.get_platform().await;
        self.get_nics().await;
        self.get_batteries();
        self.get_uptime().await;
        self.update_gfx_devices().await;
        self.update_gpu_utilization().await;
        debug!("Updated Metrics for {} processes.", self.processes.len());
    }

    pub async fn save_state(&mut self) {
        self.histogram_map.save_histograms();
    }
}
