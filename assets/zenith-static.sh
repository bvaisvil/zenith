#!/bin/sh

absPath() {
  perl -MCwd -le 'print Cwd::abs_path(shift)' "$1"
}

scriptPath="`absPath "$0"`"
scriptDir="`dirname "$scriptPath"`"

if test -r /dev/nvidia-uvm && { ldconfig -p | grep -q libnvidia-ml.so.1; }
then
  exec "$scriptDir/zenith/nvidia/zenith" "$@"
else
  exec "$scriptDir/zenith/base/zenith" "$@"
fi
