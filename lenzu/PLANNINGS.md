# Project Refactoring Plan for lenzu

Based on the current implementation of lenzu on the RemoteOCR branch, the project functions as a desktop "lens" that captures a region around the mouse cursor, processes it with OpenCV, and sends it to a remote OCR service. Currently, the logic is likely concentrated in `main.rs`, which handles window management, image processing, networking, and configuration.

To improve testability and maintainability, I recommend refactoring the project into a modular structure. This allows you to isolate side-effect-heavy code (like GUI and Networking) from pure logic (image manipulation and coordinate math), which is much easier to unit test.

## Recommended Project Structure

```plaintext
src/
├── main.rs          # Entry point, orchestrates the modules
├── config.rs        # Configuration loading and CLI arguments
├── capture.rs       # Screen capturing and cursor tracking
├── processor.rs     # OpenCV image processing logic
├── client.rs        # Remote OCR API client (Networking)
└── ui/              # Optional: UI-related code (eg. egui or tray)
    ├── mod.rs
    └── overlay.rs
```

## Refactoring Strategy by Module

### 1. processor.rs (The Core Logic)

This is where the OpenCV operations live. To make this testable, separate the "image transformation" from the "display."

**What to move**: Functions that take a `Mat` (OpenCV matrix) and return a processed `Mat` (grayscale, thresholding, resizing).

**Testing**: You can write unit tests that load a static `.png` file from a `tests/fixtures` folder, run the processor, and assert that the output dimensions or pixel values meet expectations.

### 2. client.rs (Remote OCR Communication)

Move all `reqwest` or networking logic here.

**Refactor**: Create a `OCRClient` struct that holds the endpoint URL and API keys.

**Testing**: Use the `mockito` or `wiremock` crates to simulate the Remote OCR server. This allows you to test how your app handles 200 OK, 404, or 500 Internal Server errors without actually making network calls.

### 3. config.rs

If you have hardcoded values for the lens size or OCR URL, move them to a `Config` struct.

**Refactor**: Implement `Default` for your config and a `load()` function that reads from a `.toml` or `.env` file.

**Testing**: You can test that the configuration correctly defaults when a file is missing or correctly overrides values from environment variables.

### 4. capture.rs

Isolate the logic that calculates the "Crop Area" based on cursor position.

**Refactor**: Create a function `calculate_capture_rect(cursor_pos, lens_size, screen_size) -> Rect`.

**Testing**: This is purely mathematical. You can write unit tests to ensure that if the mouse is at the very edge of the screen, the capture box doesn't go out of bounds (clamping logic).

## Implementation Example: processor.rs

By moving logic out of `main.rs`, your code becomes much cleaner:

```rust
// src/processor.rs
use opencv::prelude::*;
use opencv::imgproc;

pub struct ImageProcessor {
    pub scale_factor: f64,
}

impl ImageProcessor {
    pub fn prepare_for_ocr(&self, src: &Mat) -> opencv::Result<Mat> {
        let mut gray = Mat::default();
        imgproc::cvt_color(src, &mut gray, imgproc::COLOR_BGR2GRAY, 0)?;
        // ... more processing ...
        Ok(gray)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grayscale_conversion() {
        // Assert that a 3-channel input becomes a 1-channel output
    }
}
```

## Benefits for Unit Testing

- **Mocking**: By using traits for the `OCRClient`, you can swap the real network client for a "Mock" client during tests.
- **No GUI dependency**: You can run `cargo test` in a headless environment (like a GitHub Action) because the core logic no longer requires a live window or mouse cursor to be present.
- **Parallelism**: Rust's test runner can run tests for config, processor, and capture in parallel, speeding up your dev cycle.

## Prioritized Phases

Phase 1: Decouple Domain Logic (High Priority)
The easiest things to test are "pure" functions that don't touch the screen or the internet. Start here to see immediate value from unit tests.

1. Extract math/geometry logic:
   Task: Move the logic that calculates the "Lens" capture area (handling screen boundaries and cursor offsets) into a capture.rs or geometry.rs.
   Testing Goal: Write tests for edge cases, such as when the mouse is at (0,0) or at the far bottom-right of a 4K monitor, to ensure your crop rectangle never exceeds screen bounds.
2. Extract config handling:
   Task: Create a config.rs with a Settings struct.
   Testing Goal: Ensure that if a config file is missing, the app loads safe defaults.

Phase 2: Functional Abstraction (Medium Priority)
This step involves wrapping external dependencies (OpenCV and Reqwest) so they can be "mocked" or tested in isolation. 3. Create an ImageProcessor module:

- Task: Move all OpenCV calls (grayscale, thresholding, resizing) into processor.rs.
- Testing Goal: Use a small, hard-coded byte array or a sample .png in your tests/ folder to verify that your processing pipeline actually improves image contrast as expected.

Isolate the OCRClient:
Task: Move the reqwest logic to client.rs.
Testing Goal: Use the mockiato or wiremock crates. You want to test how your app reacts to a "429 Too Many Requests" or a malformed JSON response from the remote OCR without actually hitting the server.

Phase 3: Main Orchestration (Low Priority)
Once the logic is moved, main.rs should only be responsible for "wiring" things together.
Refactor main.rs to an App Loop:
Task: Your main function should now look like a clean list of steps:

```rust
let config = Config::load();
let frame = capture::get_screen_near_cursor(&config);
let processed = processor::prepare(frame);
let text = client.send(processed).await?;
ui::display(text);
```

Summary Table: What to test first
Module Difficulty Test Type Why?
Geometry Easy Unit Prevents "Out of Bounds" crashes on different monitor setups.
Config Easy Unit Ensures the app doesn't crash on the first run.
Processor Medium Integration Ensures OCR accuracy by verifying image preprocessing.
OCR Client Hard Mock Ensures the UI doesn't freeze if the network is down.
