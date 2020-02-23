# Zenith 
## In terminal graphical metrics for your *nix system written in Rust.
<img src="./assets/screenshot.png" alt="Running zenith under iterm on MacOS">

## Features
- Optional CPU, Memory, Network, and Disk usage histograms
- Quick glances at Disk free space, NIC IP addresses, CPU frequency
- A top like process table that includes per process disk usage
- Different histogram views (5 minutes, 1 hour, etc)
- Managing processes (signals, nice, etc)

## Planned Features
- Saving performance data
- Sensor Temperature histograms
- Per process network usage (Linux)
- Messaging about adverse system events, like errors in kernel ring buffer (Linux)
- Docker support
- More process details

## Current Platforms
- Linux
- MacOS

## Planned Platforms
- Other BSD systems may work, I have not tested.
- Perhaps Redox OS.

## Installation

Download one of the compiled releases.

## Building

This builds under rustc version 1.39.0.

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
Will remove the CPU histogram.