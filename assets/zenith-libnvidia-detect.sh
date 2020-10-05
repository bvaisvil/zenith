#!/bin/sh

LIBNAME=libnvidia-ml.so.1
EXITVALUE=1

if ldconfig -p | grep -q $LIBNAME; then
  EXITVALUE=0
else
  for path in `echo $LD_LIBRARY_PATH | sed 's/:/ /g'`; do
    if [ -r "$path/$LIBNAME" ]; then
      EXITVALUE=0
    fi
  done
fi

exit $EXITVALUE
