# Zenith 

## In terminal graphical metrics for your *nix system written in Rust

<img src="./assets/screenshot.png" alt="Running zenith on iTerm2 on MacOS">

## Features

- Optional CPU, Memory, Network, and Disk usage charts
- Quick glances at Disk free space, NIC IP addresses, CPU frequency
- Highlight top users of CPU, Memory, & Disk
- Battery percentage, time to charge or discharge, power used
- A top-like filterable process table that includes per process disk usage
- Change process priority
- Zoomable chart views (with support to scroll back in time)
- Managing processes with signals
- Performance data saved between runs
- GPU Utilization Metrics for NVIDIA GPUs (with `--features nvidia`)

## Planned Features

- CPU steal percentage and general virtualization awareness
- Sensor Temperature charts
- Per process network usage (Linux)
- Messaging about adverse system events, like errors in kernel ring buffer (Linux)
- Docker support
- ZFS (pool status)
- GPU utilization metrics for AMD GPUS
- Disk metrics like IO ops / latency
- Support Memory pressure

## Current Platforms

- Linux
- MacOS

## Planned Platforms

- BSD (OpenBSD/FreeBSD)
- Perhaps Redox OS.

## Installation

### Binary

Download one of the compiled [releases](https://github.com/bvaisvil/zenith/releases).

### Homebrew

```bash
brew install zenith
```

## Building

This builds under rustc version >= 1.39.0.

```
cd zenith
cargo build --release
```

For NVIDIA GPU support, build with feature `nvidia`:

```cargo build --release --features nvidia```

## Usage

Running with no arguments starts zenith with the default visualizations for CPU, Disk, and Network and a refresh rate of 2000 ms (2 seconds). These can be changed with command line parameters:

```USAGE:
    zenith [FLAGS] [OPTIONS]

FLAGS:
        --disable-history    Disables history when flag is present
    -h, --help               Prints help information
    -V, --version            Prints version information

OPTIONS:
    -c, --cpu-height <INT>        Height of CPU/Memory visualization. [default: 10]
        --db <STRING>             Database to use, if any. [default: ~/.zenith]
    -d, --disk-height <INT>       Height of Disk visualization. [default: 10]
    -n, --net-height <INT>        Height of Network visualization. [default: 10]
    -p, --process-height <INT>    Min Height of Process Table. [default: 8]
    -r, --refresh-rate <INT>      Refresh rate in milliseconds. [default: 2000]
```


Don't want a section? Remove it by setting the height to 0. 

For example: ```zenith -c 0``` removes the CPU chart.

Up/down arrow keys move around the process table. Return (enter) will focus on a process.
Tab switches the active section. Active sections can be expanded (e) and minimized (m). 
+/- (or =/-) will zoom in / out all of the charts. Arrow keys (←/→) move forward/backward in time.
Back tick (`) resets the chart to current time and max zoom.
Using these options you can create the layout you want.

In zenith 'h' key will show this help:

<img src="./assets/help.png" alt="Running zenith on iTerm2 on MacOS">

## Built using these great crates

- [tui-rs](https://github.com/fdehau/tui-rs)
- [sysinfo](https://github.com/GuillaumeGomez/sysinfo)
- [heim](https://github.com/heim-rs/heim)
- [battery](https://github.com/svartalf/rust-battery)
- [serde](https://github.com/serde-rs/serde)
- [clap](https://github.com/clap-rs/clap)
- [nvml-wrapper](https://github.com/Cldfire/nvml-wrapper)
