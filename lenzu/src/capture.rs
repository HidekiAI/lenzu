use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, ImageFormat};
use x11rb::rust_connection::RustConnection;

/// Returns `(width, height)` of the X11 root window (primary screen in pixels).
pub fn screen_size() -> Result<(u32, u32), Box<dyn std::error::Error>> {
    let (conn, screen_num) = RustConnection::connect(None)?;
    let screen = &conn.setup().roots[screen_num];
    Ok((screen.width_in_pixels as u32, screen.height_in_pixels as u32))
}

pub fn capture_x11(x: i32, y: i32, w: u32, h: u32) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let (conn, screen_num) = RustConnection::connect(None)?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    let reply = conn
        .get_image(
            ImageFormat::Z_PIXMAP,
            root,
            x as i16,
            y as i16,
            w as u16,
            h as u16,
            0xffffffff,
        )?
        .reply()?;

    Ok(reply.data)
}

// Unit test for X11 is usually an integration test,
// but we can at least check the connection error handling.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_x11_connection_attempt() {
        // This will pass if an X server is running, or return an Err if not.
        // It validates that our RustConnection logic doesn't panic.
        let result = capture_x11(0, 0, 1, 1);
        if std::env::var("DISPLAY").is_err() {
            assert!(result.is_err());
        }
    }
}
