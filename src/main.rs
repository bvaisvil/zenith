/**
 * Copyright 2019-2020, Benjamin Vaisvil and the zenith contributors
 */

#[macro_use]
extern crate num_derive;
#[macro_use]
extern crate log;
#[cfg(all(target_os = "linux", feature = "nvidia"))]
extern crate nvml_wrapper as nvml;

mod constants;
mod graphics;
#[cfg(not(feature = "nvidia"))]
mod graphics_none;
#[cfg(all(target_os = "linux", feature = "nvidia"))]
mod graphics_nvidia;
mod histogram;
mod metrics;
mod render;
mod util;
mod zprocess;

use crate::render::TerminalRenderer;
use gumdrop::Options;

use crossterm::{
    cursor, execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use futures::executor::block_on;
use std::error::Error;
use std::fs;
use std::io::{stdout, Write};
use std::panic;
use std::panic::PanicInfo;
use std::path::Path;
use std::process::exit;

fn panic_hook(info: &PanicInfo<'_>) {
    let location = info.location().unwrap(); // The current implementation always returns Some
    let msg = match info.payload().downcast_ref::<&'static str>() {
        Some(s) => *s,
        None => match info.payload().downcast_ref::<String>() {
            Some(s) => &s[..],
            None => "Box<Any>",
        },
    };
    error!("thread '<unnamed>' panicked at '{}', {}\r", msg, location);
    restore_terminal();
    println!("thread '<unnamed>' panicked at '{}', {}\r", msg, location);
}

fn init_terminal() {
    debug!("Initializing Terminal");
    let mut sout = stdout();
    execute!(sout, EnterAlternateScreen).expect("Unable to enter alternate screen");
    execute!(sout, cursor::Hide).expect("Unable to hide cursor");
    execute!(sout, Clear(ClearType::All)).expect("Unable to clear screen.");
    enable_raw_mode().expect("Unable to enter raw mode.");
}

fn restore_terminal() {
    debug!("Restoring Terminal");
    let mut sout = stdout();
    // Restore cursor position and clear screen for TTYs
    execute!(sout, cursor::MoveTo(0, 0)).expect("Attempt to write to alternate screen failed.");
    execute!(sout, Clear(ClearType::All)).expect("Unable to clear screen.");
    execute!(sout, LeaveAlternateScreen).expect("Unable to leave alternate screen.");
    execute!(sout, cursor::Show).expect("Unable to restore cursor.");
    disable_raw_mode().expect("Unable to disable raw mode");
}

fn start_zenith(
    rate: u64,
    cpu_height: u16,
    net_height: u16,
    disk_height: u16,
    process_height: u16,
    sensor_height: u16,
    graphics_height: u16,
    disable_history: bool,
    db_path: &str,
) -> Result<(), Box<dyn Error>> {
    debug!("Starting with Arguments: rate: {}, cpu: {}, net: {}, disk: {}, process: {}, graphics: {}, disable_history: {}, db_path: {}",
          rate,
          cpu_height,
          net_height,
          disk_height,
          process_height,
          graphics_height,
          disable_history,
          db_path
    );

    init_terminal();

    // setup a panic hook so we can see our panic messages.
    panic::set_hook(Box::new(|info| {
        panic_hook(info);
    }));

    // get pid before runtime start, so we always get the main pid and not the tid of a thread
    let main_pid = std::process::id();

    let run = || async {
        //check lock
        let (db, lock) = if !disable_history {
            let db_path = Path::new(db_path);
            if !db_path.exists() {
                debug!("Creating DB dir.");
                fs::create_dir(db_path).expect("Couldn't Create DB dir.");
            }
            debug!("Creating Lock");

            let lock_path = db_path.join(".zenith.lock");
            let lock = match util::Lockfile::new(main_pid, &lock_path).await {
                Some(f) => f,
                None => {
                    print!("{:} exists and history recording is on. Is another copy of zenith open? If not remove the path and open zenith again.", lock_path.display());
                    exit(1);
                }
            };

            // keeps the lock handle alive
            (Some(db_path.to_owned()), Some(lock))
        } else {
            (None, None)
        };

        debug!("Create Renderer");
        let mut r = TerminalRenderer::new(
            rate,
            cpu_height as i16,
            net_height as i16,
            disk_height as i16,
            process_height as i16,
            sensor_height as i16,
            graphics_height as i16,
            db,
        );

        r.start().await;

        // only drop lock at the end
        drop(lock);
    };

    block_on(run());

    debug!("Shutting Down.");

    restore_terminal();

    Ok(())
}

fn validate_refresh_rate(arg: &str) -> Result<u64, String> {
    let val = arg.parse::<u64>().map_err(|e| e.to_string())?;
    if val >= 1000 {
        Ok(val)
    } else {
        Err(format!(
            "{} Enter a refresh rate that is at least 1000 ms",
            arg
        ))
    }
}

fn default_db_path() -> String {
    dirs::cache_dir()
        .unwrap_or_else(|| Path::new("./").to_owned())
        .join("zenith")
        .to_str()
        .expect("Couldn't set default db path")
        .to_string()
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    let opts =
        ZOptions::parse_args_default(&args[1..]).map_err(|e| format!("{}: {}", args[0], e))?;

    if opts.help_requested() {
        println!(
            "zenith {}
Benjamin Vaisvil <ben@neuon.com>
Zenith, sort of like top but with histograms.
Up/down arrow keys move around the process table. Return (enter) will focus on a process.
Tab switches the active section. Active sections can be expanded (e) and minimized (m).
Using this you can create the layout you want.

Usage: {} [OPTIONS]

{}
",
            env!("CARGO_PKG_VERSION"),
            args[0],
            ZOptions::usage()
        );
        return Ok(());
    } else if opts.version {
        println!("zenith {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let graphics_height = {
        // attribute used instead of if cfg! because if cfg! does not handle conditional existence of struct fields
        #[cfg(feature = "nvidia")]
        {
            opts.graphics_height
        }
        #[cfg(not(feature = "nvidia"))]
        {
            0
        }
    };

    env_logger::init();
    info!("Starting zenith {}", env!("CARGO_PKG_VERSION"));

    start_zenith(
        opts.refresh_rate,
        opts.cpu_height,
        opts.net_height,
        opts.disk_height,
        opts.process_height,
        0,
        graphics_height,
        opts.disable_history,
        &opts.db,
    )
}

#[derive(Options)]
struct ZOptions {
    /// Disables history when flag is present
    #[options(no_short, long = "disable-history", default = "false")]
    disable_history: bool,

    #[options()]
    help: bool,

    #[options(short = "V")]
    version: bool,

    /// Height of CPU/Memory visualization.
    #[options(short = "c", long = "cpu-height", default = "10", meta = "INT")]
    cpu_height: u16,

    /// Database to use, if any.
    #[options(no_short, default_expr = "default_db_path()", meta = "STRING")]
    db: String,

    /// Height of Disk visualization.
    #[options(short = "d", long = "disk-height", default = "10", meta = "INT")]
    disk_height: u16,

    /// Height of Network visualization.
    #[options(short = "n", long = "net-height", default = "10", meta = "INT")]
    net_height: u16,

    /// Min Height of Process Table.
    #[options(short = "p", long = "process-height", default = "8", meta = "INT")]
    process_height: u16,

    /// Refresh rate in milliseconds.
    #[options(
        short = "r",
        long = "refresh-rate",
        default = "2000",
        parse(try_from_str = "validate_refresh_rate"),
        meta = "INT"
    )]
    refresh_rate: u64,

    /// Height of Graphics Card visualization.
    #[cfg(feature = "nvidia")]
    #[options(short = "g", long = "graphics-height", default = "10", meta = "INT")]
    graphics_height: u16,
}
