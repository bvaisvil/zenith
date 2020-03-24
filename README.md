# Zenith 
## In terminal graphical metrics for your *nix system written in Rust.
<img src="./assets/screenshot.png" alt="Running zenith under iterm on MacOS">

## Features
- Optional CPU, Memory, Network, and Disk usage charts
- Quick glances at Disk free space, NIC IP addresses, CPU frequency
- A top like process table that includes per process disk usage
- Different chart views (5 minutes, 1 hour, etc)
- Managing processes (signals, nice, etc)
- Saving performance data

## Planned Features
- Sensor Temperature charts
- Per process network usage (Linux)
- Messaging about adverse system events, like errors in kernel ring buffer (Linux)
- Docker support

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
brew tap bvaisvil/zenith
brew install zenith
```

## Building

This builds under rustc version >= 1.39.0.

```
cd zenith
cargo build --release
```

## Usage

Running with no arguments starts zenith with the default visualizations for CPU, Disk, and Netowrk and a refresh rate of 2000 ms (2 seconds). These can be changed with command line parameters:

```USAGE:
    zenith [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -c, --cpu-height <INT>        Height of CPU/Memory visualization. [default: 10]
    -d, --disk-height <INT>       Height of Disk visualization. [default: 10]
    -n, --net-height <INT>        Height of Network visualization. [default: 10]
    -p, --process-height <INT>    Min Height of Process Table. [default: 8]
    -r, --refresh-rate <INT>      Refresh rate in milliseconds. [default: 2000]
```


Don't want a section? Remove it by setting the height to 0. 

For example:
```zenith -c 0```
Will remove the CPU chart.

Up/down arrow keys move around the process table. Return (enter) will focus on a process.
Tab switches the active section. Active sections can be expanded (e) and minimized (m). 
+/- (or =/-) will zoom in / out all of the charts. Arrow keys (←/→) move forward/backward in time.
Back tick (`) resets the chart to current time and max zoom.
Using this you can create the layout you want.

## Built using these great crates

- [tui-rs](https://github.com/fdehau/tui-rs)
- [sysinfo](https://github.com/GuillaumeGomez/sysinfo)
- [heim](https://github.com/heim-rs/heim)
- [serde](https://github.com/serde-rs/serde)
- [clap](https://github.com/clap-rs/clap)
