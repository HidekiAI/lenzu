use anyhow::Error; // the most easiest way to handle errors
use core::result::Result;
use image::DynamicImage;
use std::{
    boxed::Box,
    fmt::{self, Display, Formatter},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct CursorData {
    pub x: i32, // cursor positions may be negative based on monitor positon relative to primary monitor (i.e. monitors left of primary monitor have negative X coordinates)
    pub y: i32,
    // info about current monitor the cursor at (x,y) is located, probably only useful for capturing the WHOLE screen
    pub monitor_x: i32, // upper left corner of the monitor relative to the PRIMARY monitor
    pub monitor_y: i32,
    pub monitor_width: u32, // rcMonitor.right - rcMonitor.left (even if both are negative, it should come out as positive) - i.e. (0 - -1024 = 1024), (-1024 - -2048 = 1024), etc
    pub monitor_height: u32,
    // current window
    pub window_x: i32, // position of the window on the monitor that is recalculated based off of cursor (x,y) and upper-left is offset by center of window to be where the mouse cursor will be
    pub window_y: i32,
    pub window_width: u32,
    pub window_height: u32,
}

impl CursorData {
    pub fn new() -> Self {
        CursorData {
            x: 0,
            y: 0,
            monitor_x: 0,
            monitor_y: 0,
            monitor_width: 1024,
            monitor_height: 768,
            window_x: 0,
            window_y: 0,
            window_width: 1,
            window_height: 1,
        }
    }
}

pub trait CaptureTrait {
    fn new() -> Self
    where
        Self: Sized;
    fn init(&self) -> bool;
    fn capture(&self, rect: CursorData) -> Result<image::DynamicImage, anyhow::Error>;
    fn update(&mut self);
}
