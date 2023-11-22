#[derive(Debug, Clone )]
pub struct Screen {
    pub index: u8,
    pub id: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub top_x: i32, // i.e.primary monitor will be (0, 0)
    pub top_y: i32, // note that you cannot assume that all monitors are aligned at top (it could be aligned at bottom) and dimensions may not be the same
}
impl Screen {
    pub fn new(index: u8, id: u32, name: &str, width: u32, height: u32, top_x: i32, top_y: i32) -> Screen {
        Screen {
            index: index,
            id: id,
            name: name.to_string(),
            width: width,
            height: height,
            top_x: top_x,
            top_y: top_y,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Desktop {
    // for mouse location related (with respect to the (possible) multiple screen)
    cursor_position: (i32, i32),
    current_screen: usize,
    screens: [Screen; 9], // [Screen], by making it Vec<Screen> instead of [Screen], we can have Clone and Copy derivation for Screen
}

impl Desktop {
    pub fn new(screens: &[Screen]) -> Desktop {
        // first, initialize 9 screens with default values
        let mut screens_mut = [
            Screen::new(0, 0, "0", 0, 0, 0, 0),
            Screen::new(1, 1, "1", 0, 0, 0, 0),
            Screen::new(2, 2, "2", 0, 0, 0, 0),
            Screen::new(3, 3, "3", 0, 0, 0, 0),
            Screen::new(4, 4, "4", 0, 0, 0, 0),
            Screen::new(5, 5, "5", 0, 0, 0, 0),
            Screen::new(6, 6, "6", 0, 0, 0, 0),
            Screen::new(7, 7, "7", 0, 0, 0, 0),
            Screen::new(8, 8, "8", 0, 0, 0, 0),
        ];
        // replace the default values with the actual values from the screens (up to 9 screens)
        for (i, screen) in screens.iter().enumerate() {
            screens_mut[i].index = i as u8;
            screens_mut[i].id = screen.id;
            screens_mut[i].name = screen.name.clone();
            screens_mut[i].width = screen.width;
            screens_mut[i].height = screen.height;
            screens_mut[i].top_x = screen.top_x;
            screens_mut[i].top_y = screen.top_y;
        }
        Desktop {
            cursor_position: (0, 0),
            current_screen: 0, // assume the first screen is the primary screen
            screens: screens_mut,
        }
    }

    // NOTE: The order of the monitors is not guaranteed to be the same as the order in which they are arranged.
    //       The order of the monitors is guaranteed to be the same as the order in which they are enumerated.
    //       The primary monitor is always the first monitor in the list.
    //       Monitor positions (origin of (0,0)) are relative to the top-left corner of the primary monitor, hence if there are 3
    //       monitors arranged in a row, in whch primary is the middle one, the left monitor will have a negative
    //       x coordinate, and the right monitor will have a positive x coordinate.
    #[cfg(target_os = "windows")]
    fn get_global_cursor_position() -> Option<(i32, i32)> {
        use std::mem;
        use winapi::shared::windef::POINT;
        use winapi::um::winuser::GetCursorPos;

        let mut point = POINT { x: 0, y: 0 };
        unsafe {
            if GetCursorPos(&mut point) != 0 {
                Some((point.x, point.y))
            } else {
                None
            }
        }
    }
    #[cfg(target_os = "linux")]
    fn get_global_cursor_position() -> Option<(i32, i32)> {
        use x11::xlib::{XCloseDisplay, XOpenDisplay, XQueryPointer, XRootWindow};

        unsafe {
            let display = XOpenDisplay(std::ptr::null());
            if display.is_null() {
                return None;
            }

            let root = XRootWindow(display, 0);
            let mut root_return = 0;
            let mut child_return = 0;
            let mut root_x_return = 0;
            let mut root_y_return = 0;
            let mut win_x_return = 0;
            let mut win_y_return = 0;
            let mut mask_return = 0;
            let query_pointer = XQueryPointer(
                display,
                root,
                &mut root_return,
                &mut child_return,
                &mut root_x_return,
                &mut root_y_return,
                &mut win_x_return,
                &mut win_y_return,
                &mut mask_return,
            );

            XCloseDisplay(display);

            if query_pointer != 0 {
                Some((root_x_return, root_y_return))
            } else {
                None
            }
        }
    }

    // suppose there are 3 screens arranged in a row, in which the primary is the middle one
    // the left screen will have a negative x coordinate, and the right screen will have a positive x coordinate
    pub fn update(&mut self) -> Screen {
        match Self::get_global_cursor_position() {
            Some(pos) => {
                self.cursor_position = pos;
                // figure out which Screen the mouse cursor is on
                // Note that position of the screen top-left can be negative due to it being relative to the primary screen
                let mut ret_screen = self.screens[self.current_screen].clone();
                for (screen_index, screen) in self.screens.iter().enumerate() {
                    if pos.0 >= screen.top_x
                        && pos.0 < screen.top_x + screen.width as i32
                        && pos.1 >= screen.top_y
                        && pos.1 < screen.top_y + screen.height as i32
                    {
                        self.current_screen = screen_index;
                        ret_screen = screen.clone();
                    }
                }
                ret_screen
            }
            None => self.screens[self.current_screen].clone(),
        }
    }

    pub fn current_screen(&self) -> Screen {
        self.screens[self.current_screen].clone()
    }   
}
