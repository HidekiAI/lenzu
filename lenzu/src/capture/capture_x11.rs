use super::capture_traits::{CaptureRect, CaptureTrait};

pub struct CaptureX11 {}

impl CaptureTrait for CaptureX11 {
    fn new() -> Self {
        CaptureX11 {}
    }

    fn init(&mut self, app_window_gtk: &gtk4::ApplicationWindow) -> bool {
        todo!()
    }

    fn capture(
        &mut self,
        possible_rect: Option<CaptureRect>,
    ) -> Result<image::DynamicImage, anyhow::Error> {
        todo!()
    }

    #[cfg(feature = "gtk")]
    fn update(&mut self) {
        todo!()
    }

    fn render(&mut self, image: image::DynamicImage) {
        todo!()
    }
}
