use super::capture_traits::{CaptureRect, CaptureTrait};

pub struct CaptureWayland {}

impl CaptureTrait for CaptureWayland {
    fn new() -> Self {
        CaptureWayland {
            cursor_data: CursorData::new(),
            window: None,
        }
    }

    fn init(&mut self, app_window_gtk: &gtk4::ApplicationWindow) -> bool {
        todo!()
    }

    fn update(&mut self) {
        todo!()
    }

    fn capture(
        &mut self,
        _possible_rect: Option<CaptureRect>,
    ) -> Result<image::DynamicImage, anyhow::Error> {
        // TODO: Implement ScreenCast portal capture.
        // This usually involves:
        // 1. Requesting a ScreenCast session via ashpd or similar.
        // 2. Handling the user's permission dialog.
        // 3. Receiving a PipeWire stream.
        // 4. Extracting a frame from the PipeWire stream.
        Err(anyhow!("Wayland capture not yet implemented. Use X11 backend for now."))
    }

    fn render(&mut self, _image: image::DynamicImage) {
        // On Wayland, we should definitely use GTK's native rendering (e.g., GtkSnapshot or GtkPicture)
        // rather than trying to draw directly to a surface handle.
        todo!("rendering image to screen is not yet implemented for wayland")
    }
}
