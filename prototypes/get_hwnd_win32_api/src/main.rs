use std::ptr;
use winapi::{
    shared::{
        minwindef::{FALSE, TRUE},
        windef::HWND,
    },
    um::{
        processthreadsapi::GetCurrentProcessId,
        winuser::{
            EnumChildWindows, EnumWindows, GetWindowTextW, GetWindowThreadProcessId,
            IsWindowVisible,
        },
    },
};

// A (so far) failed attempt at trying to extract HWND based on process ID
// this was to go around the issue of not being able to extract HWND from GTK4
// in which it is needed to work around the screen snapshot issue

// EnumChildProc callback function
// see: https://learn.microsoft.com/en-us/previous-versions/windows/desktop/legacy/ms633493(v=vs.85)
unsafe extern "system" fn enum_child_proc(hwnd: HWND, lparam: isize) -> i32 {
    *(lparam as *mut HWND) = ptr::null_mut(); // in case not found, return NULL
    if hwnd.is_null() {
        return FALSE; // we're done with enumeration
    }
    // see if we can pull out Window text for debugging purposes
    let mut window_text: [u16; 256] = [0; 256];
    let mut win_text: String = "Window Text: ''".to_string();
    GetWindowTextW(hwnd, window_text.as_mut_ptr(), window_text.len() as i32);
    if window_text[0] != 0 {
        // the text needs to be truncated on encounter of null-terminator and/or max str-length
        let window_text_utf16 = String::from_utf16_lossy(&window_text)
            .trim_matches(char::from(0))
            .to_string();
        win_text = format!(
            "Window Text: {:?}, HWND={:?}, IsVisible={}",
            window_text_utf16,
            hwnd,
            IsWindowVisible(hwnd) != FALSE
        );
    }

    let mut process_id: u32 = 0u32;
    let _thread_id = GetWindowThreadProcessId(hwnd, &mut process_id);
    let current_process = GetCurrentProcessId();
    println!( "\t{} - Child HWND={:?}, ProcessID={}", win_text, hwnd, process_id);
    if current_process != process_id {
        return TRUE; // continue enumeration/iterations
    }

    // FOUND! inject the lparam as HWND
    *(lparam as *mut HWND) = hwnd;
    FALSE // opt out of enumeration
}

// EnumWindowsProc callback function
// see: https://learn.microsoft.com/en-us/previous-versions/windows/desktop/legacy/ms633498(v=vs.85)
// we'll use lparam to be a pointer to a struct, in which the struct contains
// * process_id: DWORD (read-only)
// * hwnd: HWND (read-write) IF process_id matches
// returns FALSE to stop enumeration, TRUE to continue
unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: isize) -> i32 {
    if hwnd.is_null() {
        return FALSE;
    }

    // see if we can pull out Window text for debugging purposes
    let mut window_text: [u16; 256] = [0; 256];
    let mut win_text: String = "Window Text: ''".to_string();
    GetWindowTextW(hwnd, window_text.as_mut_ptr(), window_text.len() as i32);
    if window_text[0] != 0 {
        // the text needs to be truncated on encounter of null-terminator and/or max str-length
        let window_text_utf16 = String::from_utf16_lossy(&window_text)
            .trim_matches(char::from(0))
            .to_string();
        win_text = format!(
            "Window Text: {:?}, HWND={:?}, IsVisible={}",
            window_text_utf16,
            hwnd,
            IsWindowVisible(hwnd) != FALSE
        );
    }

    *(lparam as *mut HWND) = ptr::null_mut(); // in case not found, return NULL
    let mut process_id: u32 = 0u32;
    let _thread_id = GetWindowThreadProcessId(hwnd, &mut process_id);
    let current_process = GetCurrentProcessId();
    println!(
        "{} - Test for HWND={:?} ProcessID={} (current process: {})",
        win_text, hwnd, process_id, current_process
    );
    if process_id != current_process {
        // EnumChildWindows: if hwnd is NULL, then it will enumerate all top-level windows on the screen like EnumWindows()
        let mut child_hwnd: HWND = ptr::null_mut();
        let child_found = EnumChildWindows(
            hwnd,
            Some(enum_child_proc),
            &mut child_hwnd as *mut HWND as isize,
        );
        match child_found {
            FALSE => {
                return TRUE; // continue enumeration
            }
            _ => {
                if child_hwnd.is_null() {
                    return TRUE; // continue enumeration
                }
                println!(
                    "Found Child HWND={:?}, ProcessID={}",
                    child_hwnd, process_id
                );
                *(lparam as *mut HWND) = child_hwnd;
                return FALSE; // opt out of enumeration
            }
        }
        //return TRUE; // NOT FOUND, continue enumeration
    }
    println!("Found HWND={:?} ProcessID={}", hwnd, process_id);

    // inject the lparam as HWND
    *(lparam as *mut HWND) = hwnd;
    FALSE // FOUND: opt out of enumeration
}

fn main() {
    let current_process = unsafe { GetCurrentProcessId() };
    println!("Current process ID: {}", current_process);

    let mut hwnd: HWND = ptr::null_mut();
    unsafe {
        EnumWindows(Some(enum_windows_proc), &mut hwnd as *mut HWND as isize);
    };
    // Now 'hwnd' contains the handle of the main window
    match hwnd.is_null() {
        true => {
            println!("Main window HWND not found");
            return;
        }
        false => {
            println!("Main window HWND: {:?}", hwnd);
        }
    }
}
