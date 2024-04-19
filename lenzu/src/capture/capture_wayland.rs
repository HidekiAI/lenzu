use super::capture_traits::{CaptureRect, CaptureTrait};

pub struct CaptureWayland {}

impl CaptureTrait for CaptureWayland {
    fn new() -> Self {
        CaptureWayland {}
    }

    fn init(&mut self, app_window_gtk: &gtk4::ApplicationWindow) -> bool {
        todo!()
    }

    fn update(&mut self) {
        todo!()
    }

    fn capture(
        &mut self,
        possible_rect: Option<CaptureRect>,
    ) -> Result<image::DynamicImage, anyhow::Error> {
        todo!()
    }

    fn render(&mut self, image: image::DynamicImage) {
        todo!("rendering image to screen is not yet implemented for wayland")
    }
}
