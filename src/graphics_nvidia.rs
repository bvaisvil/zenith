/**
 * Copyright 2019-2020, Benjamin Vaisvil and the zenith contributors
 */
use crate::graphics::*;
use crate::metrics::CPUTimeApp;
use nvml::device::Device;
use nvml::enum_wrappers::device::{Clock, TemperatureSensor, TemperatureThreshold};
use nvml::struct_wrappers::device::ProcessUtilizationSample;
use nvml::NVML;
use std::convert::TryFrom;
use std::fmt;

impl From<&ProcessUtilizationSample> for GraphicsDeviceProcess {
    fn from(sample: &ProcessUtilizationSample) -> GraphicsDeviceProcess {
        GraphicsDeviceProcess {
            pid: sample.pid as i32,
            timestamp: sample.timestamp,
            sm_utilization: sample.sm_util,
            mem_utilization: sample.mem_util,
            enc_utilization: sample.enc_util,
            dec_utilization: sample.dec_util,
        }
    }
}

impl TryFrom<&Device<'_>> for GraphicsDevice {
    type Error = nvml::error::NvmlError;

    fn try_from(d: &Device<'_>) -> Result<Self, Self::Error> {
        let uuid = d.uuid()?;
        let mut gd = GraphicsDevice::new(uuid);
        match d.memory_info() {
            Ok(m) => {
                gd.total_memory = m.total;
                gd.used_memory = m.used;
            }
            Err(e) => error!("Failed Getting Memory Info {:?}", e),
        }
        gd.name = d.name().unwrap_or_default();
        gd.clock = d.clock_info(Clock::Graphics).unwrap_or(0);
        gd.max_clock = d.max_clock_info(Clock::Graphics).unwrap_or(0);
        gd.power_usage = d.power_usage().unwrap_or(0);
        gd.max_power = d.power_management_limit().unwrap_or(0);
        gd.temperature = d.temperature(TemperatureSensor::Gpu).unwrap_or(0);
        gd.temperature_max = d
            .temperature_threshold(TemperatureThreshold::GpuMax)
            .unwrap_or(0);
        for i in 0..4 {
            // get the speed of up to 5 fans;
            let r = d.fan_speed(i);
            if r.is_ok() {
                gd.fans.push(r.unwrap_or(0));
            } else {
                break;
            }
        }
        gd.decoder_utilization = match d.decoder_utilization() {
            Ok(u) => u.utilization,
            Err(_) => 0,
        };
        gd.encoder_utilization = match d.encoder_utilization() {
            Ok(u) => u.utilization,
            Err(_) => 0,
        };
        match d.utilization_rates() {
            Ok(u) => {
                gd.gpu_utilization = u.gpu;
                gd.mem_utilization = u.memory;
            }
            Err(e) => error!("Couldn't get utilization rates: {:?}", e),
        }
        match d.process_utilization_stats(None) {
            Ok(ps) => gd.processes = ps.iter().map(GraphicsDeviceProcess::from).collect(),
            Err(_) => debug!(
                "Couldn't retrieve process utilization stats for {:}",
                gd.name
            ),
        }

        Ok(gd)
    }
}

impl fmt::Display for GraphicsDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: GPU: {}% MEM: {}%",
            self.name, self.gpu_utilization, self.mem_utilization
        )
    }
}

impl GraphicsExt for CPUTimeApp {
    fn update_gfx_devices(&mut self) {
        self.gfx_devices.clear();
        let nvml = NVML::init();
        let n = match nvml {
            Ok(n) => n,
            Err(e) => {
                error!("Couldn't init NVML: {:?}", e);
                return;
            }
        };

        let count = n.device_count().unwrap_or(0);
        for i in 0..count {
            let d = match n.device_by_index(i) {
                Ok(d) => d,
                Err(e) => {
                    error!("Couldn't get gfx device at index: {}: {:?}", i, e);
                    continue;
                }
            };
            let gd = match GraphicsDevice::try_from(&d) {
                Ok(gd) => gd,
                Err(e) => {
                    error!("Failed Getting Memory Info {:?}", e);
                    continue;
                }
            };
            self.histogram_map.add_value_to(
                format!("{}_gpu", gd.uuid).as_str(),
                gd.gpu_utilization as u64,
            );
            self.histogram_map.add_value_to(
                format!("{}_mem", gd.uuid).as_str(),
                gd.mem_utilization as u64,
            );

            debug!("{:}", gd);
            // mock device code to test multiple cards.
            //let mut gd2 = gd.clone();
            self.gfx_devices.push(gd);
            //
            //gd2.name = String::from("Card2");
            //gd2.max_clock = 1000;
            //self.gfx_devices.push(gd2);
        }
    }

    fn update_gpu_utilization(&mut self) {
        for d in &mut self.gfx_devices {
            for p in &d.processes {
                let proc = self.process_map.get_mut(&p.pid);
                if let Some(proc) = proc {
                    proc.gpu_usage =
                        (p.sm_utilization + p.dec_utilization + p.enc_utilization) as u64;
                    proc.fb_utilization = p.mem_utilization as u64;
                    proc.dec_utilization = p.dec_utilization as u64;
                    proc.enc_utilization = p.enc_utilization as u64;
                    proc.enc_utilization = p.enc_utilization as u64;
                }
            }
        }
    }
}
