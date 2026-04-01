#!/bin/bash
# Setup for Linux (and in future, MinGW64) to install 3rd party libs that `cargo build` needs
# that could not be installed via crates dependencies.

sudo apt update
sudo apt install -y \
    libgtk-4-dev    \
    libgdk-4-dev    \
    libcairo2-dev   \
    libpango1.0-dev     \
    libgraphene-1.0-dev     \
    libgraphene-gobject-1.0-dev     
