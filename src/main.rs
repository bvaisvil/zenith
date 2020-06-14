/**
 * Copyright 2019 Benjamin Vaisvil
 */

#[macro_use]
extern crate num_derive;
#[macro_use]
extern crate log;
#[cfg(all(target_os = "linux", feature = "nvidia"))]
extern crate nvml_wrapper as nvml;

mod constants;
mod histogram;
mod metrics;
mod render;
mod util;
mod zprocess;

use crate::render::TerminalRenderer;
use clap::{App, Arg};

use futures::executor::block_on;
use std::error::Error;
use std::fs;
use std::io::{stdout, Write};
use std::panic;
use std::panic::PanicInfo;
use std::path::Path;
use std::process::exit;
use termion::input::MouseTerminal;
use termion::raw::IntoRawMode;
use termion::screen::AlternateScreen;
use tui::backend::TermionBackend;
use tui::Terminal;

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
    println!(
        "{}thread '<unnamed>' panicked at '{}', {}\r",
        termion::screen::ToMainScreen,
        msg,
        location
    );
}

fn init_terminal() {
    debug!("Initializing Terminal");
    let raw_term = stdout()
        .into_raw_mode()
        .expect("Could not bind to STDOUT in raw mode.");
    debug!("Create Mouse Term");
    let mouse_term = MouseTerminal::from(raw_term);
    debug!("Create Alternate Screen");
    let mut screen = AlternateScreen::from(mouse_term);
    debug!("Clear Screen");
    // Need to clear screen for TTYs
    write!(screen, "{}", termion::clear::All)
        .expect("Attempt to write to alternate screen failed.");
}

fn restore_terminal() {
    debug!("Restoring Terminal");
    let raw_term = stdout()
        .into_raw_mode()
        .expect("Could not bind to STDOUT in raw mode.");
    let mut screen = AlternateScreen::from(raw_term);
    // Restore cursor position and clear screen for TTYs
    write!(
        screen,
        "{}{}",
        termion::cursor::Goto(1, 1),
        termion::clear::All
    )
    .expect("Attempt to write to alternate screen failed.");
    let backend = TermionBackend::new(screen);
    let mut terminal = Terminal::new(backend).expect("Could not create new terminal.");
    terminal.show_cursor().expect("Restore cursor failed.");
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
    let default_db_path = dirs::cache_dir()
        .unwrap_or_else(|| Path::new("./").to_owned())
        .join("zenith");
    let default_db_path = default_db_path
        .to_str()
        .expect("Couldn't set default db path");
    let mut args = vec![
        Arg::with_name("refresh-rate")
            .short("r")
            .long("refresh-rate")
            .value_name("INT")
            .default_value("2000")
            .validator(validate_refresh_rate)
            .help("Refresh rate in milliseconds.")
            .takes_value(true),
        Arg::with_name("cpu-height")
            .short("c")
            .long("cpu-height")
            .value_name("INT")
            .default_value("10")
            .validator(validate_height)
            .help("Height of CPU/Memory visualization.")
            .takes_value(true),
        Arg::with_name("net-height")
            .short("n")
            .long("net-height")
            .value_name("INT")
            .default_value("10")
            .validator(validate_height)
            .help("Height of Network visualization.")
            .takes_value(true),
        Arg::with_name("disk-height")
            .short("d")
            .long("disk-height")
            .value_name("INT")
            .default_value("10")
            .validator(validate_height)
            .help("Height of Disk visualization.")
            .takes_value(true),
        Arg::with_name("process-height")
            .short("p")
            .long("process-height")
            .value_name("INT")
            .default_value("8")
            .validator(validate_height)
            .help("Min Height of Process Table.")
            .takes_value(true),
        Arg::with_name("disable-history")
            .long("disable-history")
            .help("Disables history when flag is present")
            .takes_value(false),
        Arg::with_name("db")
            .long("db")
            .value_name("STRING")
            .default_value(default_db_path)
            .help("Database to use, if any.")
            .takes_value(true),
    ];
    if cfg!(feature = "nvidia") {
        args.push(
            Arg::with_name("graphics-height")
                .short("g")
                .long("graphics-height")
                .value_name("INT")
                .default_value("10")
                .validator(validate_height)
                .help("Height of Graphics Card visualization.")
                .takes_value(true),
        );
    }
    let matches = App::new("zenith")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Benjamin Vaisvil <ben@neuon.com>")
        .about(
            "Zenith, sort of like top but with histograms.
Up/down arrow keys move around the process table. Return (enter) will focus on a process.
Tab switches the active section. Active sections can be expanded (e) and minimized (m).
Using this you can create the layout you want.",
        )
        .args(args.as_slice())
        .get_matches();

    let graphics_height = if cfg!(feature = "nvidia") {
        matches
            .value_of("graphics-height")
            .unwrap()
            .parse::<u16>()
            .unwrap()
    } else {
        0
    };

    env_logger::init();
    info!("Starting zenith {}", env!("CARGO_PKG_VERSION"));

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
        0,
        graphics_height,
        matches.is_present("disable-history"),
        matches.value_of("db").unwrap(),
    )
}
