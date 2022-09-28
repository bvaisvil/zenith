use super::style::{max_style, ok_style};
/**
 * Copyright 2019-2022, Benjamin Vaisvil and the zenith contributors
 */
use super::{percent_of, Render, ZBackend, LEFT_PANE_WIDTH};
use crate::float_to_byte_string;
use crate::metrics::histogram::{HistogramKind, View};
use crate::metrics::CPUTimeApp;
use byte_unit::{Byte, ByteUnit};
use std::borrow::Cow;
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::{Color, Modifier, Style};
use tui::text::{Span, Spans};
use tui::widgets::{Block, Borders, List, ListItem, Sparkline};
use tui::Frame;

pub fn render_graphics(
    app: &CPUTimeApp,
    layout: Rect,
    f: &mut Frame<'_, ZBackend>,
    view: View,
    gfx_device_index: &usize,
    border_style: Style,
) {
    Block::default()
        .title("Graphics")
        .borders(Borders::ALL)
        .border_style(border_style)
        .render(f, layout);
    let gfx_layout = Layout::default()
        .margin(0)
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(LEFT_PANE_WIDTH), Constraint::Min(10)].as_ref())
        .split(layout);
    let area = Layout::default()
        .margin(1)
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(gfx_layout[1]);
    if app.gfx_devices.is_empty() {
        return;
    }

    let view = View {
        width: area[0].width as usize,
        ..view
    };

    let gd = &app.gfx_devices[*gfx_device_index];
    let h_gpu = match app
        .histogram_map
        .get_zoomed(&HistogramKind::GpuUse(gd.uuid.clone()), &view)
    {
        Some(h) => h,
        None => return,
    };
    let fan = if !gd.fans.is_empty() {
        format!("FAN [{:3.0}%]", gd.fans[0])
    } else {
        String::from("")
    };

    #[cfg(not(all(target_os = "linux", feature = "nvidia")))]
    let version = "";
    #[cfg(all(target_os = "linux", feature = "nvidia"))]
    let version = if let (Some(dv), Some(cv), Some(nv)) = (
        &app.nvml_driver_version,
        &app.nvml_cuda_version,
        &app.nvml_version,
    ) {
        format!(" VER [DRIVER: {:} CUDA: {:} NVML: {:}]", dv, cv, nv)
    } else {
        format!("")
    };
    Sparkline::default()
        .block(
            Block::default().title(
                format!(
                    "GPU [{:3.0}%] ENC [{:3.0}%] DEC [{:3.0}%] PROC [{:}] CLOCK [{:} Mhz / {:} Mhz]{:}",
                    gd.gpu_utilization,
                    gd.encoder_utilization,
                    gd.decoder_utilization,
                    gd.processes.len(),
                    gd.clock,
                    gd.max_clock,
                    version
                )
                .as_str(),
            ),
        )
        .data(h_gpu.data())
        .style(Style::default().fg(Color::LightYellow))
        .max(100)
        .render(f, area[0]);

    let h_mem = match app
        .histogram_map
        .get_zoomed(&HistogramKind::GpuMem(gd.uuid.clone()), &view)
    {
        Some(h) => h,
        None => return,
    };

    Sparkline::default()
        .block(Block::default().title(Spans(vec![
                Span::raw(
                    format!(
                        "FB [{:3.0}%] MEM [{:} / {:} - {:}%] {:}",
                        gd.mem_utilization,
                        float_to_byte_string!(gd.used_memory as f64, ByteUnit::B),
                        float_to_byte_string!(gd.total_memory as f64, ByteUnit::B),
                        percent_of(gd.used_memory, gd.total_memory) as u64,
                        fan,
                    )
                    .as_str(),
                ),
                Span::raw(" PWR ["),
                Span::styled(
                    format!("{:} W / {:} W", gd.power_usage / 1000, gd.max_power / 1000),
                    if gd.power_usage > gd.max_power {
                        max_style()
                    } else {
                        ok_style()
                    },
                ),
                Span::raw("] TEMP ["),
                Span::styled(
                    format!("{:} C / {:} C", gd.temperature, gd.temperature_max),
                    if gd.temperature > gd.temperature_max {
                        max_style()
                    } else {
                        ok_style()
                    },
                ),
                Span::raw("]"),
            ])))
        .data(h_mem.data())
        .style(Style::default().fg(Color::LightMagenta))
        .max(100)
        .render(f, area[1]);
    let devices: Vec<_> = app
        .gfx_devices
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let indicator = if i == *gfx_device_index { "→" } else { " " };
            let style = if d.gpu_utilization > 90 {
                max_style()
            } else {
                ok_style()
            };
            Span::styled(
                Cow::Owned(format!(
                    "{}{:3.0}%: {}",
                    indicator, d.gpu_utilization, d.name
                )),
                style,
            )
        })
        .map(ListItem::new)
        .collect();
    List::new(devices)
        .block(
            Block::default()
                .title(Span::styled("Graphics Devices", border_style))
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .render(f, gfx_layout[0]);
}
