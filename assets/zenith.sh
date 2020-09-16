#!/bin/sh

PREFIX=/usr/local

if test -r /dev/nvidia-uvm && { ldconfig -p | grep -q libnvidia-ml.so.1; }
then
  exec $PREFIX/lib/zenith/nvidia/zenith "$@"
else
  exec $PREFIX/lib/zenith/base/zenith "$@"
fi
