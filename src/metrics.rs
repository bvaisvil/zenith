/**
 * Copyright 2019 Benjamin Vaisvil (ben@neuon.com)
 */
use sysinfo::{Disk, NetworkExt, System, SystemExt, ProcessorExt, DiskExt, Pid, ProcessExt, Process, ProcessStatus};
use crate::zprocess::*;
use std::collections::{HashMap, HashSet};
use users::{User, UsersCache, Users, Groups};
use std::time::SystemTime;

#[derive(FromPrimitive, PartialEq, Copy, Clone)]
pub enum ProcessTableSortBy{
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
    Cmd = 10
}

pub trait DiskFreeSpaceExt{
    fn get_perc_free_space(&self) -> f64;
}

impl DiskFreeSpaceExt for Disk{
    fn get_perc_free_space(&self) -> f64{
        if self.get_total_space() < 1{
            return 0.0;
        }
        ((self.get_available_space() as f64) / (self.get_total_space() as f64)) * 100.00
    }
}

pub struct CPUTimeApp<'a> {
    pub cpu_usage_histogram: Vec<u64>,
    pub cpu_utilization: u64,
    pub mem_utilization: u64,
    pub mem_total: u64,
    pub mem_usage_histogram: Vec<u64>,
    pub swap_utilization: u64,
    pub swap_total: u64,
    pub disks: Vec<Disk>,
    pub disk_total: u64,
    pub disk_available: u64,
    pub disk_write: u64,
    pub disk_read: u64,
    pub disk_read_histogram: Vec<u64>,
    pub disk_write_histogram: Vec<u64>,
    pub cpus: Vec<(String, u64)>,
    pub system: System,
    pub overview: Vec<(&'a str, u64)>,
    pub net_in: u64,
    pub net_in_histogram: Vec<u64>,
    pub net_out_histogram: Vec<u64>,
    pub net_out: u64,
    pub processes: Vec<i32>,
    pub process_map: HashMap<i32, ZProcess>,
    pub user_cache: UsersCache,
    pub cum_cpu_process: Option<i32>,
    pub frequency: u64,
    pub highlighted_row: usize,
    pub threads_total: usize,
    pub psortby: ProcessTableSortBy
}

impl<'a> CPUTimeApp<'a>{
    pub fn new () -> CPUTimeApp<'a>{
        let mut s = CPUTimeApp{
            cpu_usage_histogram: Vec::with_capacity(60),
            mem_usage_histogram: Vec::with_capacity(60),
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
            overview: vec![
                ("CPU", 0),
                ("MEM", 0),
                ("SWP", 0),
                ("DSK", 0)
            ],
            net_in: 0,
            net_in_histogram: Vec::with_capacity(60),
            net_out: 0,
            net_out_histogram: Vec::with_capacity(60),
            processes: Vec::with_capacity(400),
            process_map: HashMap::with_capacity(400),
            user_cache: UsersCache::new(),
            cum_cpu_process: Option::from(0),
            frequency: 0,
            highlighted_row: 0,
            threads_total: 0,
            disk_read: 0,
            disk_write: 0,
            disk_read_histogram: Vec::with_capacity(60),
            disk_write_histogram: Vec::with_capacity(60),
            psortby: ProcessTableSortBy::DiskRead
        };
        s.system.refresh_all();
        s.system.refresh_all();
        return s
    }

    pub fn highlight_up(&mut self){
        if self.highlighted_row != 0{
            self.highlighted_row -= 1;
        }
    }

    pub fn highlight_down(&mut self){
        if self.highlighted_row < self.process_map.len(){
            self.highlighted_row += 1;
        }
    }

    fn update_process_list(&mut self){
        self.processes.clear();
        let process_list = self.system.get_process_list();
        let mut current_pids: HashSet<i32> = HashSet::with_capacity(process_list.len());
        let mut top_pid = 0;
        let mut top_cum_cpu_usage: f64 = 0.0;
        self.threads_total = 0;

        for (pid, process) in process_list{
            if self.process_map.contains_key(pid){
                let zp = self.process_map.get_mut(pid).unwrap();
                zp.memory = process.memory();
                zp.cpu_usage = process.cpu_usage();
                zp.cum_cpu_usage += zp.cpu_usage as f64;
                zp.status = process.status();
                zp.priority = process.priority;
                zp.virtual_memory = process.virtual_memory;
                zp.threads_total = process.threads_total;
                self.threads_total += zp.threads_total as usize;
                if zp.cum_cpu_usage > top_cum_cpu_usage{
                    top_pid = zp.pid;
                    top_cum_cpu_usage = zp.cum_cpu_usage;
                }
                zp.prev_read_bytes = zp.read_bytes;
                zp.prev_write_bytes = zp.write_bytes;
                zp.read_bytes = process.read_bytes;
                zp.write_bytes = process.write_bytes;
                zp.last_updated = SystemTime::now();
            }
            else{
                let user_name = match self.user_cache.get_user_by_uid(process.uid){
                    Some(user) => user.name().to_string_lossy().to_string(),
                    None => String::from("")
                };
                let zprocess = ZProcess{
                    uid: process.uid,
                    user_name: user_name,
                    pid: pid.clone(),
                    memory: process.memory(),
                    cpu_usage: process.cpu_usage(),
                    command: process.cmd().to_vec(),
                    status: process.status(),
                    exe: format!("{}", process.exe().display()),
                    name: process.name().to_string(),
                    cum_cpu_usage: process.cpu_usage() as f64,
                    priority: process.priority,
                    virtual_memory: process.virtual_memory,
                    threads_total: process.threads_total,
                    read_bytes: process.read_bytes,
                    write_bytes: process.write_bytes,
                    prev_read_bytes: process.read_bytes,
                    prev_write_bytes: process.write_bytes,
                    last_updated: SystemTime::now()
                };
                self.threads_total += zprocess.threads_total as usize;
                if zprocess.cum_cpu_usage > top_cum_cpu_usage{
                    top_pid = zprocess.pid;
                    top_cum_cpu_usage = zprocess.cum_cpu_usage;
                }
                self.process_map.insert(zprocess.pid, zprocess);
            }
            self.processes.push(pid.clone());
            current_pids.insert(pid.clone());
        }

        // remove pids that are gone
        self.process_map.retain(|&k, _| current_pids.contains(&k));

        
        self.sort_process_table();
        self.cum_cpu_process = Option::from(top_pid);
    }

    pub fn sort_process_table(&mut self){
        let pm = &self.process_map;
        let sortfield = &self.psortby;
        self.processes.sort_by(|a, b| {
            let pa =pm.get(a).unwrap();
            let pb = pm.get(b).unwrap();
            match sortfield{
                ProcessTableSortBy::CPU => pb.cpu_usage.partial_cmp(&pa.cpu_usage).unwrap(),
                ProcessTableSortBy::Mem=> pb.memory.partial_cmp(&pa.memory).unwrap(),
                ProcessTableSortBy::MemPerc=> pb.memory.partial_cmp(&pa.memory).unwrap(),
                ProcessTableSortBy::User => pa.user_name.partial_cmp(&pb.user_name).unwrap(),
                ProcessTableSortBy::Pid => pa.pid.partial_cmp(&pb.pid).unwrap(),
                ProcessTableSortBy::Status => pa.status.to_single_char().partial_cmp(pb.status.to_single_char()).unwrap(),
                ProcessTableSortBy::Priority => pa.priority.partial_cmp(&pb.priority).unwrap(),
                ProcessTableSortBy::Virt=> pb.virtual_memory.partial_cmp(&pa.virtual_memory).unwrap(),
                ProcessTableSortBy::Cmd => pb.name.partial_cmp(&pa.name).unwrap(),
                ProcessTableSortBy::DiskRead => pb.get_read_bytes_sec().partial_cmp(&pa.get_read_bytes_sec()).unwrap(),
                ProcessTableSortBy::DiskWrite => pb.get_write_bytes_sec().partial_cmp(&pa.get_write_bytes_sec()).unwrap()
            }

        });
    }

    fn update_frequency(&mut self){
        self.frequency = sys_info::cpu_speed().unwrap_or(0);
    }

    fn update_disk(&mut self, width: u16){
        self.disk_available = 0;
        self.disk_total = 0;
        self.disks.clear();
        for d in self.system.get_disks().iter(){
            self.disk_available += d.get_available_space();
            self.disk_total += d.get_total_space();
            self.disks.push(d.clone());
        }

        let du = self.disk_total - self.disk_available;
        self.overview[3] = ("DSK", ((du as f32 / self.disk_total as f32) * 100.0) as u64);
        self.disk_read = self.process_map.iter().map(|(pid, p)| p.get_read_bytes_sec() as u64).sum();
        self.disk_write = self.process_map.iter().map(|(pid, p)| p.get_write_bytes_sec() as u64).sum();

        self.disk_read_histogram.push(self.disk_read);
        if self.disk_read_histogram.len() > (width - 2) as usize{
            self.disk_read_histogram.remove(0);
        }

        self.disk_write_histogram.push(self.disk_write);
        if self.disk_write_histogram.len() > (width - 2) as usize{
            self.disk_write_histogram.remove(0);
        }
    }

    pub fn update(&mut self, width: u16) {
        self.system.refresh_all();
        let procs = self.system.get_processor_list();
        let mut num_procs = 1;
        let mut usage: f32 = 0.0;
        self.cpus.clear();
        for p in procs.iter().skip(1){
            let u = p.get_cpu_usage();
            self.cpus.push((format!("{}", num_procs), (u * 100.0) as u64));
            usage += u;
            num_procs += 1;
        }
        let usage = usage / num_procs as f32;
        self.cpu_utilization = (usage * 100.0) as u64;
        self.overview[0] = ("CPU", self.cpu_utilization);
        self.cpu_usage_histogram.push((usage * 100.0) as u64);
        if self.cpu_usage_histogram.len() > (width -2) as usize{
            self.cpu_usage_histogram.remove(0);
        }

        self.mem_utilization = self.system.get_used_memory();
        self.mem_total = self.system.get_total_memory();

        let mut mem: u64 = 0;
        if self.mem_total > 0{
            mem = ((self.mem_utilization as f64/ self.mem_total as f64) * 100.0) as u64;
        }


        self.overview[1] = ("MEM", mem);
        self.mem_usage_histogram.push(mem);
        if self.mem_usage_histogram.len() > (width - 2) as usize{
            self.mem_usage_histogram.remove(0);
        }

        self.swap_utilization = self.system.get_used_swap();
        self.swap_total = self.system.get_total_swap();


        let mut swp: u64 = 0;
        if self.swap_total > 0 && self.swap_utilization > 0{
            swp = ((self.swap_utilization as f64/ self.swap_total as f64) * 100.0) as u64;
        }
        self.overview[2] = ("SWP", swp);



        let net = self.system.get_network();

        self.net_in = net.get_income();
        self.net_out = net.get_outcome();
        self.net_in_histogram.push(self.net_in);
        self.net_out_histogram.push(self.net_out);
        while self.net_in_histogram.len() > (width - 2) as usize{
            self.net_in_histogram.remove(0);
        }
        while self.net_out_histogram.len() > (width - 2) as usize{
            self.net_out_histogram.remove(0);
        }
        self.update_process_list();
        self.update_frequency();
        self.update_disk(width);
    }
}