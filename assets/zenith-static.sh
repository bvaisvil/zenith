#!/bin/sh

absPath() {
  perl -MCwd -le 'print Cwd::abs_path(shift)' "$1"
}

scriptPath="`absPath "$0"`"
scriptDir="`dirname "$scriptPath"`"

if test -r /dev/nvidiactl && "$scriptDir/zenith-exec/zenith-libnvidia-detect"; then
  exec "$scriptDir/zenith-exec/nvidia/zenith" "$@"
else
  exec "$scriptDir/zenith-exec/base/zenith" "$@"
fi
