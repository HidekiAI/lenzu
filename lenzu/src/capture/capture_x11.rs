use super::capture_traits::{CaptureRect, CaptureTrait, CursorData};
use anyhow::{anyhow, Result};
use image::{DynamicImage, GenericImageView, RgbaImage};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{GContext, ImageFormat};
use x11rb::rust_connection::RustConnection;

#[cfg(feature = "gtk")]
use gdk::prelude::*;
#[cfg(feature = "gtk")]
use gdkx11::X11Window;

pub struct CaptureX11 {
    pub cursor_data: CursorData,
    pub conn: Option<RustConnection>,
    pub screen_num: usize,
    #[cfg(feature = "gtk")]
    pub window: Option<glib::WeakRef<gtk::ApplicationWindow>>,
    pub xid: Option<x11rb::protocol::xproto::Window>,
    pub gc: Option<GContext>,
}

impl CaptureTrait for CaptureX11 {
    fn new() -> Self {
        CaptureX11 {
            cursor_data: CursorData::new(),
            conn: None,
            screen_num: 0,
            #[cfg(feature = "gtk")]
            window: None,
            xid: None,
            gc: None,
        }
    }
    #[cfg(feature = "gtk")]
    fn init(&mut self, app_window_gtk: &gtk::ApplicationWindow) -> bool {
        self.window = Some(app_window_gtk.downgrade());

        let surface = app_window_gtk.window();
        if let Some(x11_surface) = surface.and_then(|s| s.downcast::<X11Window>().ok()) {
            self.xid = Some(x11_surface.xid() as x11rb::protocol::xproto::Window);
        }

        match x11rb::connect(None) {
            Ok((conn, screen_num)) => {
                if let Some(xid) = self.xid {
                    if let Ok(gc) = conn.generate_id() {
                        if let Ok(_) = conn.create_gc(gc, xid, &Default::default()) {
                            self.gc = Some(gc);
                        }
                    }
                }
                self.conn = Some(conn);
                self.screen_num = screen_num;
                true
            }
            Err(e) => {
                eprintln!("Failed to connect to X11 server: {}", e);
                false
            }
        }
    }

    #[cfg(not(feature = "gtk"))]
    fn init(&mut self, _app_window_gtk: &super::capture_traits::dummy_types::DummyWindow) -> bool {
        true
    }

    fn capture(
        &mut self,
        possible_rect: Option<CaptureRect>,
    ) -> Result<image::DynamicImage, anyhow::Error> {
        let conn = self
            .conn
            .as_ref()
            .ok_or_else(|| anyhow!("Not connected to X11"))?;
        let screen = &conn.setup().roots[self.screen_num];
        let root = screen.root;

        let rect = possible_rect.unwrap_or(CaptureRect {
            x: self.cursor_data.monitor.x,
            y: self.cursor_data.monitor.y,
            width: self.cursor_data.monitor.width,
            height: self.cursor_data.monitor.height,
        });

        // XGetImage equivalent
        let reply = conn
            .get_image(
                ImageFormat::Z_PIXMAP,
                root,
                rect.x as i16,
                rect.y as i16,
                rect.width as u16,
                rect.height as u16,
                0xffffffff,
            )?
            .reply()?;

        // Convert raw data (usually BGRA on X11) to DynamicImage
        let width = rect.width;
        let height = rect.height;
        let data = swap_red_blue(reply.data);

        let img = RgbaImage::from_raw(width, height, data)
            .ok_or_else(|| anyhow!("Failed to create image from raw data"))?;

        Ok(DynamicImage::ImageRgba8(img))
    }

    #[cfg(feature = "gtk")]
    fn update(&mut self) {
        if let Some(window) = self.window.as_ref().and_then(|w| w.upgrade()) {
            let display = window.get_display();
            let seat = display.default_seat().unwrap();
            let pointer = seat.pointer().unwrap();

            let (pos_x, pos_y) = pointer.get_position();
            self.cursor_data.x = pos_x as i32;
            self.cursor_data.y = pos_y as i32;

            if let Some(monitor) = display.monitor_at_point(pos_x as i32, pos_y as i32) {
                let geom = monitor.workarea();
                self.cursor_data.monitor.x = geom.x();
                self.cursor_data.monitor.y = geom.y();
                self.cursor_data.monitor.width = geom.width() as u32;
                self.cursor_data.monitor.height = geom.height() as u32;
            }

            // Update application window size
            let width = window.get_width();
            let height = window.get_height();
            self.cursor_data.window.width = width as u32;
            self.cursor_data.window.height = height as u32;

            // Recalculate window position to center on cursor
            self.cursor_data.window.x = self.cursor_data.x - (width / 2);
            self.cursor_data.window.y = self.cursor_data.y - (height / 2);
        }
    }

    #[cfg(not(feature = "gtk"))]
    fn update(&mut self) {}
    fn render(&mut self, image: image::DynamicImage) {
        if let (Some(conn), Some(xid), Some(gc)) = (&self.conn, self.xid, self.gc) {
            let (width, height) = image.dimensions();
            let rgba = image.to_rgba8();
            let data = swap_red_blue(rgba.into_raw());

            let _ = conn.put_image(
                ImageFormat::Z_PIXMAP,
                xid,
                gc,
                width as u16,
                height as u16,
                0,
                0,
                0,
                24, // depth
                &data,
            );
            let _ = conn.flush();
        }
    }
}

pub fn swap_red_blue(mut data: Vec<u8>) -> Vec<u8> {
    for chunk in data.chunks_exact_mut(4) {
        let b = chunk[0];
        let r = chunk[2];
        chunk[0] = r;
        chunk[2] = b;
        chunk[3] = 255;
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swap_red_blue() {
        let input = vec![10, 20, 30, 0]; // B, G, R, X
        let expected = vec![30, 20, 10, 255]; // R, G, B, A
        let output = swap_red_blue(input);
        assert_eq!(output, expected);
    }

    #[test]
    fn test_capture_x11_new() {
        let capture = CaptureX11::new();
        assert!(capture.conn.is_none());
        #[cfg(feature = "gtk")]
        {
            assert!(capture.window.is_none());
        }
        assert_eq!(capture.screen_num, 0);
    }

    #[test]
    fn test_cursor_data_initialization() {
        let capture = CaptureX11::new();
        assert_eq!(capture.cursor_data.x, 0);
        assert_eq!(capture.cursor_data.y, 0);
    }

    #[test]
    fn test_capture_integration_mock() {
        // We simulate what GetImage would return: BGRA data
        let width = 2;
        let height = 2;
        let mock_bgra_data = vec![
            10, 20, 30, 0, // Pixel 1: B=10, G=20, R=30
            40, 50, 60, 0, // Pixel 2: B=40, G=50, R=60
            70, 80, 90, 0, // Pixel 3
            100, 110, 120, 0, // Pixel 4
        ];

        // This test validates that our "Integration" logic (swap_red_blue + RgbaImage::from_raw)
        // produces the correct DynamicImage regardless of a real X11 connection.
        let processed_data = swap_red_blue(mock_bgra_data);
        let img_result = RgbaImage::from_raw(width, height, processed_data);

        assert!(img_result.is_some());
        let img = img_result.unwrap();
        let pixel = img.get_pixel(0, 0);

        // Should be converted to RGBA
        assert_eq!(pixel[0], 30); // R
        assert_eq!(pixel[1], 20); // G
        assert_eq!(pixel[2], 10); // B
        assert_eq!(pixel[3], 255); // A (Forced opaque)
    }
}
