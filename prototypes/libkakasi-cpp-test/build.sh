#!/bin/bash
cd build
rm CMakeCache.txt 
cmake .. 
make --trace
find . -executable
