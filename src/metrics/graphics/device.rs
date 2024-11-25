/**
 * Copyright 2019-2020, Benjamin Vaisvil and the zenith contributors
 */

pub trait GraphicsExt {
    fn update_gfx_devices(&mut self);
    #[allow(dead_code)]
    fn update_total(&mut self, total: Option<GraphicsDevice>);
    fn update_gpu_utilization(&mut self);
}

#[allow(dead_code)]
#[derive(Clone)]
pub struct GraphicsDeviceProcess {
    pub pid: i32,
    pub timestamp: u64,
    pub sm_utilization: u32,
    pub mem_utilization: u32,
    pub enc_utilization: u32,
    pub dec_utilization: u32,
}

#[derive(Clone)]
pub struct GraphicsDevice {
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
    pub processes: Vec<GraphicsDeviceProcess>,
}

impl GraphicsDevice {
    #[allow(dead_code)]
    pub(crate) fn new(uuid: String) -> GraphicsDevice {
        GraphicsDevice {
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
}
