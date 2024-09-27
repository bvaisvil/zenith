/**
 * Copyright 2019-2020, Benjamin Vaisvil and the zenith contributors
 */
use crate::metrics::graphics::device::GraphicsDevice;
use crate::metrics::graphics::device::GraphicsExt;
use crate::metrics::CPUTimeApp;

impl GraphicsExt for CPUTimeApp {
    fn update_gfx_devices(&mut self) {}
    fn update_total(&mut self, mut total: Option<GraphicsDevice>) {}
    fn update_gpu_utilization(&mut self) {}
}
