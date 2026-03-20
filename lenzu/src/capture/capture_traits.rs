// TODO: Rename "Capture" to become "Desktop" (or "Screen" or "Display") to handle both screen capture (read from)  and screen rendering (write to)
use anyhow::Error; // the most easiest way to handle errors
use core::result::Result;
use image::DynamicImage;
use std::{
    boxed::Box,
    fmt::{self, Display, Formatter},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleTypes {
    #[cfg(all(feature = "gtk", target_os = "windows"))]
    Win32Handle(gdk4_win32::HWND),
    #[cfg(not(feature = "gtk"))]
    MockHandle,
    //Xwin( gdk4_wayland::HANDLE),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CaptureRect {
    pub x: i32, // on some Desktops, this can be negative if primary monitor sits right or bottom of another monitor
    pub y: i32,
    pub width: u32,
    pub height: u32,
}
impl CaptureRect {
    pub fn new() -> Self {
        CaptureRect {
            x: 0,
            y: 0,
            width: 1024,
            height: 768,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct CursorData {
    pub x: i32, // cursor positions may be negative based on monitor positon relative to primary monitor (i.e. monitors left of primary monitor have negative X coordinates)
    pub y: i32,

    // info about current monitor the cursor at (x,y) is located, probably only useful for capturing the WHOLE screen
    // rcMonitor.right - rcMonitor.left (even if both are negative, it should come out as positive) - i.e. (0 - -1024 = 1024), (-1024 - -2048 = 1024), etc
    pub monitor: CaptureRect, // upper left corner of the monitor relative to the PRIMARY monitor

    // current window
    // position of the window on the monitor that is recalculated based off of cursor (x,y) and upper-left is offset by center of window to be where the mouse cursor will be
    pub window: CaptureRect,
}

impl CursorData {
    pub fn new() -> Self {
        CursorData {
            x: 0,
            y: 0,
            monitor: CaptureRect::new(),
            window: CaptureRect::new(),
        }
    }
}

pub trait CaptureTrait {
    fn new() -> Self
    where
        Self: Sized;

    // IMPORTANT:  init() attempts to extract gdk4_<desktop>::Surface::Handle() (i.e. HWND),
    // it MUST be called AFTER the window has been presented() so that the GdkSurface exists!!!
    #[cfg(feature = "gtk")]
    fn init(&mut self, app_window_gtk: &gtk4::ApplicationWindow) -> bool;

    #[cfg(not(feature = "gtk"))]
    fn init(&mut self, app_window_gtk: &dummy_types::DummyWindow) -> bool;
    fn capture(
        &mut self,
        possible_rect: Option<CaptureRect>,
    ) -> Result<image::DynamicImage, anyhow::Error>;
    fn update(&mut self);
    fn render(&mut self, image: DynamicImage);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capture_rect_new() {
        let rect = CaptureRect::new();
        assert_eq!(rect.x, 0);
        assert_eq!(rect.y, 0);
        assert_eq!(rect.width, 1024);
        assert_eq!(rect.height, 768);
    }

    #[test]
    fn test_cursor_data_new() {
        let data = CursorData::new();
        assert_eq!(data.x, 0);
        assert_eq!(data.y, 0);
        assert_eq!(data.monitor.width, 1024);
    }

    #[test]
    fn test_negative_coordinate_math() {
        // Mock a scenario where a monitor is left of the primary monitor
        let mut data = CursorData::new();
        data.monitor.x = -1920; // 1920px offset to the left
        data.monitor.y = 0;
        data.monitor.width = 1920;
        data.monitor.height = 1080;
        
        // Ensure our math for relative positioning works
        let relative_x = data.x - data.monitor.x;
        assert_eq!(relative_x, 1920); // Relative X within the secondary monitor
    }
}

#[cfg(test)]
pub mod dummy_types {
    pub struct DummyWindow;
}
