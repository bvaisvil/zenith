/**
 * Copyright 2019 Benjamin Vaisvil
 */
extern crate num;
extern crate num_traits;

extern crate sysinfo;
#[macro_use]
extern crate num_derive;

mod constants;
mod metrics;
mod render;
mod util;
mod zprocess;

use crate::render::TerminalRenderer;
use clap::{App, Arg};
use futures::executor::block_on;
use std::error::Error;
use std::io;
use std::panic;
use std::panic::PanicInfo;
use termion::input::MouseTerminal;
use termion::raw::IntoRawMode;
use termion::screen::AlternateScreen;
use tui::backend::TermionBackend;
use tui::Terminal;
use sled;
use dirs;
use std::path::{Path};

fn panic_hook(info: &PanicInfo<'_>) {
    let location = info.location().unwrap(); // The current implementation always returns Some
    let msg = match info.payload().downcast_ref::<&'static str>() {
        Some(s) => *s,
        None => match info.payload().downcast_ref::<String>() {
            Some(s) => &s[..],
            None => "Box<Any>",
        },
    };
    println!(
        "{}thread '<unnamed>' panicked at '{}', {}\r",
        termion::screen::ToMainScreen,
        msg,
        location
    );
}

fn start_zenith(
    rate: u64,
    cpu_height: u16,
    net_height: u16,
    disk_height: u16,
    process_height: u16,
    sensor_height: u16,
) -> Result<(), Box<dyn Error>> {
    // Terminal initialization
    let stdout = io::stdout()
        .into_raw_mode()
        .expect("Could not bind to STDOUT in raw mode.");
    let stdout = MouseTerminal::from(stdout);
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend).expect("Could not create new terminal.");
    terminal.hide_cursor().expect("Hiding cursor failed.");
    let db = sled::open(dirs::home_dir().unwrap_or(Path::new("./").to_owned()).join(".zenith"))?;
    panic::set_hook(Box::new(|info| {
        panic_hook(info);
    }));
    let mut r = TerminalRenderer::new(rate,
                                      cpu_height as i16,
                                      net_height as i16,
                                      disk_height as i16,
                                      process_height as i16,
                                      sensor_height as i16,
                                                    db
    );
    Ok(block_on(r.start()))
}

fn validate_refresh_rate(arg: String) -> Result<(), String> {
    let val = arg.parse::<u64>().unwrap_or(0);
    if val >= 1000 {
        Ok(())
    } else {
        Err(format!(
            "{} Enter a refresh rate that is at least 1000 ms",
            &*arg
        ))
    }
}

fn validate_height(arg: String) -> Result<(), String> {
    let val = arg.parse::<i64>().unwrap_or(0);
    if val >= 0 {
        Ok(())
    } else {
        Err(format!(
            "{} Enter a height greater than or equal to 0.",
            &*arg
        ))
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let matches = App::new("zenith")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Benjamin Vaisvil <ben@neuon.com>")
        .about("Like htop but with histograms.")
        .arg(
            Arg::with_name("refresh-rate")
                .short("r")
                .long("refresh-rate")
                .value_name("INT")
                .default_value("2000")
                .validator(validate_refresh_rate)
                .help(format!("Refresh rate in milliseconds.").as_str())
                .takes_value(true),
        )
        .arg(
            Arg::with_name("cpu-height")
                .short("c")
                .long("cpu-height")
                .value_name("INT")
                .default_value("10")
                .validator(validate_height)
                .help(format!("Height of CPU/Memory visualization.").as_str())
                .takes_value(true),
        )
        .arg(
            Arg::with_name("net-height")
                .short("n")
                .long("net-height")
                .value_name("INT")
                .default_value("10")
                .validator(validate_height)
                .help(format!("Height of Network visualization.").as_str())
                .takes_value(true),
        )
        .arg(
            Arg::with_name("disk-height")
                .short("d")
                .long("disk-height")
                .value_name("INT")
                .default_value("10")
                .validator(validate_height)
                .help(format!("Height of Disk visualization.").as_str())
                .takes_value(true),
        )
        // .arg(
        //     Arg::with_name("sensor-height")
        //         .short("s")
        //         .long("sensor-height")
        //         .value_name("INT")
        //         .default_value("10")
        //         .validator(validate_height)
        //         .help(format!("Height of Sensor visualization.").as_str())
        //         .takes_value(true),
        // )
        .arg(
            Arg::with_name("process-height")
                .short("p")
                .long("process-height")
                .value_name("INT")
                .default_value("8")
                .validator(validate_height)
                .help(format!("Min Height of Process Table.").as_str())
                .takes_value(true),
        )
        .get_matches();

    start_zenith(
        matches
            .value_of("refresh-rate")
            .unwrap()
            .parse::<u64>()
            .unwrap(),
        matches
            .value_of("cpu-height")
            .unwrap()
            .parse::<u16>()
            .unwrap(),
        matches
            .value_of("net-height")
            .unwrap()
            .parse::<u16>()
            .unwrap(),
        matches
            .value_of("disk-height")
            .unwrap()
            .parse::<u16>()
            .unwrap(),
        matches
            .value_of("process-height")
            .unwrap()
            .parse::<u16>()
            .unwrap(),
        0    
        // matches
        //     .value_of("sensor-height")
        //     .unwrap()
        //     .parse::<u16>()
        //     .unwrap(),
    )
}
