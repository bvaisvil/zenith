#!/bin/sh

if test -r /dev/nvidia-uvm && { ldconfig -p | grep -q libnvidia-ml.so.1; }
then
  exec zenith.nvidia "$@"
else
  exec zenith.base "$@"
fi
