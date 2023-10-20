#!/bin/bash

_BUILD_DIR=./build
_VCPKG_DIR=/usr/src/github/vcpkg/

if ! [ -e ${_VCPKG_DIR} ] ; then
    echo "Install vcpkg via 'git clone https://github.com/Microsoft/vcpkg.git' in dir ${_VCPKG_DIR} (and make sure to run 'bootstrap-vcpkg.sh')"
    exit -1
fi

if ! [ -e ${_BUILD_DIR} ]; then
    mkdir -p ${_BUILD_DIR}
fi

${_VCPKG_DIR}/vcpkg install opencv
cmake -B ${_BUILD_DIR} -S . -DCMAKE_TOOLCHAIN_FILE=${_VCPKG_DIR}/scripts/buildsystems/vcpkg.cmake
cmake --build ${_BUILD_DIR}

