#!/bin/bash
if [ ! -d build ]; then
    mkdir build
fi
cd build
if [ -f CMakeCache.txt ]; then
    rm CMakeCache.txt
fi

# NOTE:  in case ninja is installed we'll make sure to force it to use Makefile instead of ninja.build
cmake -G "Unix Makefiles" .. 
make --trace
find . -executable
