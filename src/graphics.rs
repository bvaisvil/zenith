/**
 * Copyright 2019 Benjamin Vaisvil
 */
use std::fmt;

#[derive(Clone)]
pub struct GFXDeviceProcess {
    pub pid: i32,
    pub timestamp: u64,
    pub sm_utilization: u32,
    pub mem_utilization: u32,
    pub enc_utilization: u32,
    pub dec_utilization: u32,
}

impl GFXDeviceProcess {
    #[cfg(all(target_os = "linux", feature = "nvidia"))]
    fn from_nvml(process: &ProcessUtilizationSample) -> GFXDeviceProcess {
        GFXDeviceProcess {
            pid: process.pid as i32,
            timestamp: process.timestamp,
            sm_utilization: process.sm_util,
            mem_utilization: process.mem_util,
            enc_utilization: process.enc_util,
            dec_utilization: process.dec_util,
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
    pub processes: Vec<GFXDeviceProcess>,
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
            processes: vec![],
        }
    }

    #[cfg(all(target_os = "linux", feature = "nvidia"))]
    fn processes_from_nvml(&mut self, processes: Vec<ProcessUtilizationSample>) {
        self.processes = processes
            .iter()
            .map(|p| GFXDeviceProcess::from_nvml(p))
            .collect()
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
