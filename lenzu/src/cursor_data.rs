use winapi::{
    shared::windef::RECT,
    um::winuser::{GetCursorPos, GetMonitorInfoW, GetWindowRect, MonitorFromPoint, PostQuitMessage, MONITORINFO, MONITOR_DEFAULTTONEAREST},
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct CursorData {
    x: i32, // cursor positions may be negative based on monitor positon relative to primary monitor (i.e. monitors left of primary monitor have negative X coordinates)
    y: i32,
    // info about current monitor the cursor at (x,y) is located, probably only useful for capturing the WHOLE screen
    monitor_x: i32, // upper left corner of the monitor relative to the PRIMARY monitor
    monitor_y: i32,
    monitor_width: u32, // rcMonitor.right - rcMonitor.left (even if both are negative, it should come out as positive) - i.e. (0 - -1024 = 1024), (-1024 - -2048 = 1024), etc
    monitor_height: u32,
    // current window
    window_x: i32, // position of the window on the monitor that is recalculated based off of cursor (x,y) and upper-left is offset by center of window to be where the mouse cursor will be
    window_y: i32,
    window_width: u32,
    window_height: u32,
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

    pub fn update(&mut self, application_window_handle: winapi::shared::windef::HWND) {
        // first, get cursor position so that we can dtermine which monitor we are on
        let mut cursor_pos = winapi::shared::windef::POINT { x: 0, y: 0 };
        if unsafe { GetCursorPos(&mut cursor_pos) } == 0 {
            // Handle the error appropriately if necessary.
            println!("Could not get cursor position");
            // post quit
            unsafe { PostQuitMessage(-1) };
        }
        self.x = cursor_pos.x;
        self.y = cursor_pos.y;

        // Get dimension of the monitor the cursor is currently on (see MonitorFromPoint()) via GetMonitorInfoW()
        let h_monitor = unsafe { MonitorFromPoint(cursor_pos, MONITOR_DEFAULTTONEAREST) };
        let mut monitor_info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            rcMonitor: RECT {
                // display area rectangle
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            rcWork: RECT {
                // work area rectangle (rectangle not obscured by taskbar and toolbar)
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            dwFlags: 0,
        };

        unsafe {
            GetMonitorInfoW(h_monitor, &mut monitor_info);
        }
        // note that we use rcWork rectangle, so that we can ignore the taskbar and toolbar
        // work area, unlike monitor area, is usually/should-be positive because it's the area that's not obscured by the taskbar and toolbar
        self.monitor_x = monitor_info.rcWork.left; // can be negative, based on being placed LEFT of the PRIMARY monitor
        self.monitor_y = monitor_info.rcWork.top;
        self.monitor_width =
            std::cmp::max(monitor_info.rcWork.right - monitor_info.rcWork.left, 1024) as u32;
        self.monitor_height =
            std::cmp::max(monitor_info.rcWork.bottom - monitor_info.rcWork.top, 768) as u32;

        // get current dimension of the windown on the monitor via via GetWindowRect() (GetWindowInfo() can do the same, but it provides more info that we care...)
        let mut window_rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // get the dimension of the application window
        unsafe {
            GetWindowRect(application_window_handle, &mut window_rect);
        }
        self.window_width = (window_rect.right - window_rect.left) as u32;
        self.window_height = (window_rect.bottom - window_rect.top) as u32;
        // window position (upper left corner) is recalculated based off of cursor (x,y) and upper-left is offset by center of window to be where the mouse cursor will be
        // the tricky part of this is that the windows position coordinate can be negative (same as cursor position) so it's not possible to test for min()/max() for
        // edge of the monitor, and so we'll not do snap to monitor and allow windows to get beyond the edges of the monitors
        self.window_x = self.x - (self.window_width as i32 / 2) as i32;
        self.window_y = self.y - (self.window_height as i32 / 2) as i32;
    }
    
    pub(crate) fn window_width(&self) -> u32 {
        self.window_width
    }
    pub(crate) fn window_height(&self) -> u32 {
        self.window_height
    }
    
    pub(crate) fn window_x(&self) -> i32 {
        self.window_x
    }
    pub(crate) fn window_y(&self) -> i32 {
        self.window_y
    }
}
