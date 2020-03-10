/**
 * Copyright 2019 Benjamin Vaisvil
 */
use crate::zprocess::*;
use chrono;
use futures::StreamExt;
use heim::host;
use heim::net;
use heim::net::Address;
use heim::{sensors};
use std::collections::{HashMap, HashSet};
use std::mem::swap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::cmp::Ordering::{Equal};

use sysinfo::{ComponentExt, Disk, DiskExt, NetworkExt, ProcessExt, ProcessorExt, System, SystemExt, Process};
use users::{Users, UsersCache};
use sled;
use std::path::PathBuf;
use std::borrow::Borrow;
use serde_derive::{Serialize, Deserialize};
use bincode;
use sysinfo::Signal::Sys;
use std::ops::Sub;

const ONE_WEEK: u64 = 60*60*24*7;
const DB_ERROR: &str = "Couldn't open database.";
const DSER_ERROR: &str = "Couldn't deserialize object";
const SER_ERROR: &str = "Couldn't serialize object";

#[derive(FromPrimitive, PartialEq, Copy, Clone)]
pub enum ProcessTableSortBy {
    Pid = 0,
    User = 1,
    Priority = 2,
    CPU = 3,
    MemPerc = 4,
    Mem = 5,
    Virt = 6,
    Status = 7,
    DiskRead = 8,
    DiskWrite = 9,
    Cmd = 10,
}

#[derive(PartialEq, Eq)]
pub enum ProcessTableSortOrder {
    Ascending = 0,
    Descending = 1,
}

pub trait DiskFreeSpaceExt {
    fn get_perc_free_space(&self) -> f64;
}

impl DiskFreeSpaceExt for Disk {
    fn get_perc_free_space(&self) -> f64 {
        if self.get_total_space() < 1 {
            return 0.0;
        }
        ((self.get_available_space() as f64) / (self.get_total_space() as f64)) * 100.00
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

#[derive(Serialize, Deserialize, Debug)]
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

pub struct HistogramMap {
    map: HashMap<String, Histogram>,
    duration: Duration,
    tick: Duration,
    time: SystemTime,
    db: Option<sled::Db>
}

impl HistogramMap {
    fn new(dur: Duration, tick: Duration, db: Option<sled::Db>) -> HistogramMap {
        let mut map = HashMap::with_capacity(10);
        let current_time = SystemTime::now();
        let mut tick = tick;
        match &db{
            Some(db) => {
                
                let previous_stop = match db.get("stop_time").expect(DB_ERROR){
                    Some(t) => bincode::deserialize(&t).expect(DSER_ERROR),
                    None => current_time
                };

                let tick = match db.get("tick").expect(DB_ERROR){
                    Some(t) => bincode::deserialize(&t).expect(DSER_ERROR),
                    None => tick
                };
                
                if previous_stop < current_time{
                    let d = current_time.duration_since(previous_stop)
                                                .expect("Current time is before stored time. This should not happen.");

                    // restore previous histograms
                    for k in db.iter().keys(){
                        let k = k.expect(DB_ERROR);
                        let key = String::from_utf8(k.to_owned().to_vec()).expect("Couldn't make a utf8 string from that key.");
                        if k == "stop_time"{ continue; }
                        if k == "tick"{ continue; }
                        if k == "start_time" { continue; }
                        match db.get(&k).expect(DB_ERROR){
                            Some(v) => {
                                let mut v: Histogram = bincode::deserialize(&v).expect(format!("while loading previous data: {:?}", k).as_str());
                                let mut dur = Duration::from(d);
                                // add 0s between then and now.
                                let zero_dur = Duration::from_secs(0);
                                while dur > zero_dur + tick{
                                    v.data.push(0);
                                    dur -= tick;
                                }
                                map.insert(key, v);
                                
                            },
                            None => {}
                        }
                    }
                }
            },
            None => {

            }
        }

        HistogramMap {
            map,
            duration: dur,
            tick,
            time: current_time,
            db
        }
    }

    fn add(&mut self, name: &str) -> &mut Histogram {
        let size = (self.duration.as_secs() / self.tick.as_secs()) as usize; //smallest has to be >= 1000ms
        let names = name.to_owned();
        let r = self.map.insert(names, Histogram::new(size));
        self.get_mut(name) .expect("Unexpectedly couldn't get mutable reference to value we just added.")
    }

    pub fn get_zoomed(&self, name: &str, zoom_factor: u32, width: usize) -> Option<Histogram> {
        match self.get(name) {
            Some(h) => {
                let mut nh = Histogram::new(width);
                let last_index = h.data.len();
                let nh_len = nh.data.len();
                let mut si: usize = last_index;
                for index in (0..nh_len).rev() {
                    if si < zoom_factor as usize {
                        break;
                    }
                    for i in si - zoom_factor as usize..si {
                        if i >= 0 {
                            nh.data[index] += h.data[i];
                        }
                    }

                    si -= zoom_factor as usize;
                }

                nh.data = nh.data.iter().map(|d| d / zoom_factor as u64).collect();
                Some(nh)
            }
            None => None,
        }
    }

    pub fn get(&self, name: &str) -> Option<&Histogram> {
        let v = self.map.get(name);
        v
    }

    fn get_mut(&mut self, name: &str) -> Option<&mut Histogram> {
        let v = self.map.get_mut(name);
        v
    }

    fn has(&self, name: &str) -> bool {
        self.map.contains_key(name)
    }

    fn add_value_to(&mut self, name: &str, val: u64) {
        let h: &mut Histogram = match self.has(name) {
            true => self.get_mut(name).expect("Couldn't get mutable reference"),
            false => self.add(name),
        };
        h.data.push(val);
    }

    pub fn hist_duration(&self, width: usize, zoom_factor: u32) -> chrono::Duration {
        chrono::Duration::from_std(Duration::from_secs_f64(
            self.tick.as_secs_f64() * width as f64 * zoom_factor as f64,
        ))
        .expect("Unexpectedly large duration was out of range.")
    }

    fn save_histograms(&mut self){
        match &self.db{
            Some(db) => {
                let now = SystemTime::now();
                db.insert("stop_time".as_bytes(), bincode::serialize(&now).expect(SER_ERROR)).expect(DB_ERROR);
                db.insert("tick".as_bytes(), bincode::serialize(&self.tick).expect(SER_ERROR)).expect(DB_ERROR);
                for k in self.map.keys(){
                    match self.get(k){
                        Some(v) => {db.insert(k.as_bytes(), bincode::serialize(&v).expect(SER_ERROR)).expect(DB_ERROR);},
                        None => {}
                    }
                }
            },
            None => {}
        }
        
    }
}

impl Drop for HistogramMap{
    fn drop(&mut self){
        self.save_histograms();
    }
}

pub struct CPUTimeApp {
    pub histogram_map: HistogramMap,
    pub cpu_utilization: u64,
    pub mem_utilization: u64,
    pub mem_total: u64,
    pub swap_utilization: u64,
    pub swap_total: u64,
    pub disks: Vec<Disk>,
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
    pub frequency: u64,
    pub highlighted_row: usize,
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
    pub tick: Duration,
    pub processor_name: String,
    pub started: chrono::DateTime<chrono::Local>,
    pub selected_process: Option<ZProcess>,
}

impl CPUTimeApp {
    pub fn new(tick: Duration, db: Option<sled::Db>) -> CPUTimeApp {

        let mut s = CPUTimeApp {
            histogram_map: HistogramMap::new(Duration::from_secs(60 * 60 * 24), tick, db),
            tick,
            cpus: vec![],
            system: System::new(),
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
            highlighted_row: 0,
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
        };
        s.system.refresh_all();
        s.system.refresh_all(); // apparently multiple refreshes are necessary to fill in all values.

        return s;
    }

    async fn get_platform(&mut self) {
        match host::platform().await {
            Ok(p) => {
                self.osname = p.system().to_owned();
                self.arch = p.architecture().as_str().to_owned();
                self.hostname = p.hostname().to_owned();
                self.version = p.version().to_owned();
                self.release = p.release().to_owned();
            },
            Err(_) => {
                self.osname = String::from("unknown");
                self.arch = String::from("unknown");
                self.hostname = String::from("unknown");
                self.version = String::from("unknown");
                self.release = String::from("unknown");
            }
        };
    }

    async fn get_nics(&mut self) {
        self.network_interfaces.clear();
        let mut nics = net::nic();
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
                    if ip.len() == 0 {
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
                        ip: ip,
                        dest: dest,
                    });
                },
                Err(_) => println!("Couldn't get information on a nic")
            }
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

    pub fn highlight_up(&mut self) {
        if self.highlighted_row != 0 {
            self.highlighted_row -= 1;
        }
    }

    pub fn highlight_down(&mut self) {
        if self.highlighted_row < self.process_map.len() {
            self.highlighted_row += 1;
        }
    }

    pub fn select_process(&mut self) {
        match self.processes.get(self.highlighted_row) {
            Some(p) => match self.process_map.get(p){
                Some(p) => self.selected_process = Some(p.clone()),
                None => self.selected_process = None
            }
            None => self.selected_process = None
        };
    }

    fn copy_to_zprocess(&self, process: &Process) -> ZProcess{
        let user_name = match self.user_cache.get_user_by_uid(process.uid) {
            Some(user) => user.name().to_string_lossy().to_string(),
            None => String::from(""),
        };
        ZProcess {
            uid: process.uid,
            user_name: user_name,
            pid: process.pid().clone(),
            memory: process.memory(),
            cpu_usage: process.cpu_usage(),
            command: process.cmd().to_vec(),
            status: process.status(),
            exe: format!("{}", process.exe().display()),
            name: process.name().to_string(),
            cum_cpu_usage: process.cpu_usage() as f64,
            priority: process.priority,
            virtual_memory: process.virtual_memory(),
            threads_total: process.threads_total,
            read_bytes: process.read_bytes,
            write_bytes: process.write_bytes,
            prev_read_bytes: process.read_bytes,
            prev_write_bytes: process.write_bytes,
            last_updated: SystemTime::now(),
            end_time: None,
            start_time: process.start_time(),
        }
    }

    fn update_process_list(&mut self) {
        self.processes.clear();
        let process_list = self.system.get_process_list();
        let mut current_pids: HashSet<i32> = HashSet::with_capacity(process_list.len());
        let mut top_pid: Option<i32> = None;
        let mut top_cum_cpu_usage: f64 = match &self.cum_cpu_process{
            Some(p) => p.cum_cpu_usage,
            None => 0.0
        };
        self.threads_total = 0;

        for (pid, process) in process_list {
            if let Some(zp) = self.process_map.get_mut(pid) {
                if zp.start_time == process.start_time(){
                    zp.memory = process.memory();
                    zp.cpu_usage = process.cpu_usage();
                    zp.cum_cpu_usage += zp.cpu_usage as f64;
                    zp.status = process.status();
                    zp.priority = process.priority;
                    zp.virtual_memory = process.virtual_memory();
                    zp.threads_total = process.threads_total;
                    self.threads_total += zp.threads_total as usize;
                    if zp.cum_cpu_usage > top_cum_cpu_usage {
                        top_pid = Some(zp.pid);
                        top_cum_cpu_usage = zp.cum_cpu_usage;
                    }
                    zp.prev_read_bytes = zp.read_bytes;
                    zp.prev_write_bytes = zp.write_bytes;
                    zp.read_bytes = process.read_bytes;
                    zp.write_bytes = process.write_bytes;
                    zp.last_updated = SystemTime::now();
                }
                else{
                    let zprocess = self.copy_to_zprocess(&process);
                    self.threads_total += zprocess.threads_total as usize;
                    if zprocess.cum_cpu_usage > top_cum_cpu_usage {
                        top_pid = Some(zprocess.pid);
                        top_cum_cpu_usage = zprocess.cum_cpu_usage;
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
                self.process_map.insert(zprocess.pid, zprocess);
            }
            self.processes.push(pid.clone());
            current_pids.insert(pid.clone());
        }


        // remove pids that are gone
        self.process_map.retain(|&k, _| current_pids.contains(&k));

        //set top cumulative process if we've changed it.
        match top_pid{
            Some(p) => {
                match self.process_map.get(&p){
                    Some(p) => self.cum_cpu_process = Some(p.clone()),
                    None => ()
                }
            },
            None => {
                match &mut self.cum_cpu_process{
                    Some(p) => {
                        if self.process_map.contains_key(&p.pid){
                            match self.process_map.get(&p.pid){
                                Some(cp) => {
                                    if cp.start_time == p.start_time {
                                        self.cum_cpu_process = Some(cp.clone());
                                    } else {
                                        p.set_end_time();
                                    }


                                },
                                None => ()
                            }
                        }
                        else{
                            // our cumulative winner is dead
                            p.set_end_time();
                        }
                    },
                    None => ()
                }
            }
        };

        // update selected process
        if let Some(p) = self.selected_process.as_mut(){
            let pid = &p.pid;
            if self.process_map.contains_key(pid) {
                self.selected_process = Some(self.process_map[pid].clone());
            } else {
                p.set_end_time();
            }
        }

        self.sort_process_table();
    }

    pub fn sort_process_table(&mut self) {
        let pm = &self.process_map;
        let sortfield = &self.psortby;
        let sortorder = &self.psortorder;
        self.processes.sort_by(|a, b| {
            let mut pa = pm.get(a).expect("Error in sorting the process table.");
            let mut pb = pm.get(b).expect("Error in sorting the process table.");
            match sortorder {
                ProcessTableSortOrder::Ascending => {
                    //do nothing
                }
                ProcessTableSortOrder::Descending => {
                    swap(&mut pa, &mut pb);
                }
            }
            match sortfield {
                ProcessTableSortBy::CPU => pa.cpu_usage.partial_cmp(&pb.cpu_usage).unwrap_or(Equal),
                ProcessTableSortBy::Mem => pa.memory.partial_cmp(&pb.memory).unwrap_or(Equal),
                ProcessTableSortBy::MemPerc => pa.memory.partial_cmp(&pb.memory).unwrap_or(Equal),
                ProcessTableSortBy::User => pa.user_name.partial_cmp(&pb.user_name).unwrap_or(Equal),
                ProcessTableSortBy::Pid => pa.pid.partial_cmp(&pb.pid).unwrap_or(Equal),
                ProcessTableSortBy::Status => pa
                    .status
                    .to_single_char()
                    .partial_cmp(pb.status.to_single_char())
                    .unwrap_or(Equal),
                ProcessTableSortBy::Priority => pa.priority.partial_cmp(&pb.priority).unwrap_or(Equal),
                ProcessTableSortBy::Virt => {
                    pa.virtual_memory.partial_cmp(&pb.virtual_memory).unwrap_or(Equal)
                }
                ProcessTableSortBy::Cmd => pa.name.partial_cmp(&pb.name).unwrap_or(Equal),
                ProcessTableSortBy::DiskRead => pa
                    .get_read_bytes_sec()
                    .partial_cmp(&pb.get_read_bytes_sec())
                    .unwrap_or(Equal),
                ProcessTableSortBy::DiskWrite => pa
                    .get_write_bytes_sec()
                    .partial_cmp(&pb.get_write_bytes_sec())
                    .unwrap_or(Equal),
            }
        });
    }

    fn update_frequency(&mut self) {
        self.frequency = sys_info::cpu_speed().unwrap_or(0);
    }

    fn update_disk(&mut self, width: u16) {
        self.disk_available = 0;
        self.disk_total = 0;
        self.disks.clear();
        for d in self.system.get_disks().iter() {
            self.disk_available += d.get_available_space();
            self.disk_total += d.get_total_space();
            self.disks.push(d.clone());
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
        let procs = self.system.get_processor_list();
        let mut num_procs = 0;
        let mut usage: f32 = 0.0;
        self.cpus.clear();
        let mut usagev: Vec<f32> = vec![];
        for (i, p) in procs.iter().enumerate() {
            if i == 0 {
                self.processor_name = p.get_name().to_owned();
                continue;
            }
            let mut u = p.get_cpu_usage();
            if u.is_nan() {
                u = 0.0;
            }
            self.cpus
                .push((format!("{}", num_procs + 1), (u * 100.0) as u64));
            usage += u;
            usagev.push(u);
            num_procs += 1;
        }
        if num_procs == 0 {
            self.cpu_utilization = 0;
        } else {
            usage = usage / num_procs as f32;
            self.cpu_utilization = (usage * 100.0) as u64;
        }
        self.histogram_map
            .add_value_to("cpu_usage_histogram", self.cpu_utilization);
    }

    pub async fn update(&mut self, width: u16) {
        self.system.refresh_all();
        self.update_cpu().await;
        //self.update_sensors().await;

        self.mem_utilization = self.system.get_used_memory();
        self.mem_total = self.system.get_total_memory();

        let mut mem: u64 = 0;
        if self.mem_total > 0 {
            mem = ((self.mem_utilization as f64 / self.mem_total as f64) * 100.0) as u64;
        }

        self.histogram_map.add_value_to("mem_utilization", mem);

        self.swap_utilization = self.system.get_used_swap();
        self.swap_total = self.system.get_total_swap();

        let net = self.system.get_network();

        self.net_in = net.get_income();
        self.net_out = net.get_outcome();
        self.histogram_map.add_value_to("net_in", self.net_in);
        self.histogram_map.add_value_to("net_out", self.net_out);
        self.update_process_list();
        self.update_frequency();
        self.update_disk(width);
        self.get_platform().await;
        self.get_nics().await;
    }

    pub async fn save_state(&mut self){
        self.histogram_map.save_histograms();
    }
}
