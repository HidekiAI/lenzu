# Lenzu: Japanese OCR Screen Lens

Lenzu is a high-performance, real-time screen-capture and OCR utility written in Rust. It provides a "magnifying lens" that follows the mouse cursor, allowing the user to capture and transcribe Japanese text from any window (including browsers and hardware-accelerated apps) using the Gemini 2.0 Flash API via OpenRouter.

## 🚀 Accomplishments & Features

### 1. Advanced Screen Capture

- **X11 Root Capture**: Bypasses window-specific limitations by capturing directly from the X11 root window.
- **Hardware Acceleration Bypass**: Successfully captures text from GPU-accelerated browsers (like Yahoo.co.jp news) that traditional screenshot tools often miss.
- **Pixel-Perfect Alignment**: The capture area is mathematically centered on the mouse cursor, ensuring the green boundary box perfectly matches the OCR input.

### 2. UI & UX (GTK3 + Cairo + Pango)

- **Dynamic Lens**: A 400x400 "floating" window that follows the cursor smoothly.
- **Transparency & Alpha Support**: Uses RGBA visuals for a modern "frosted glass" UI panel with 85% opacity.
- **Japanese Font Support**: Implemented Pango rendering to solve the "tofu" (square boxes) issue, ensuring Japanese characters display correctly in the UI.
- **Visual Feedback**:
  - **Cyan Border**: Clearly defines the 400x400 capture zone.
  - **Camera Flash**: A white "shutter" fade effect triggers on capture for instant feedback.
  - **Animated Spinner**: A CSS-style spinning arc indicates when an API call is in progress.

### 3. Core Logic & Threading

- **Thread-Safe API Calls**: The heavy `reqwest` calls are moved to a background thread to prevent UI freezing, using `glib::MainContext::channel` to send results back to the main loop.
- **Automated Clipboard**: OCR results are automatically copied to the system clipboard (`arboard`) upon success.
- **Persistent Logging**: Every successful transcription is logged with a timestamp to `/dev/shm/ocr_history.txt`.
- **Debug Mode**: Automatically saves the last raw capture to `/dev/shm/debug_lens.png` for rapid troubleshooting.

## 🛠 Technical Stack

- **Language**: Rust (Edition 2021)
- **UI Toolkit**: GTK 0.18 (via gtk-rs)
- **Drawing**: Cairo & Pango
- **Protocol**: x11rb (X11 Rust Bindings)
- **AI Model**: `google/gemini-2.0-flash-001` (via OpenRouter)

## ⌨️ Controls

- **Mouse Move**: Lens follows cursor.
- **Shift + Left Click**: Trigger Capture, Flash, and OCR.
- **Esc**: Quit the application immediately.
- **Ctrl + Q**: Alternative Quit shortcut.

## 📋 Environment Requirements

The app expects the following environment variable:

```bash
export OPENROUTER_API_KEY=sk-your-key-here
```

Required system fonts for Japanese rendering:

```bash
sudo apt-get install fonts-noto-cjk fonts-ipafont-gothic
```

## 🏗 Future Context for Next Session

- **Alignment Status**: The math for `win_x`/`win_y` is now 1:1 with the capture box.
- **Threading Status**: Using `glib` channels for thread safety; GTK objects never leave the main thread.
- **Prompting**: Current prompt is optimized for "OCR ONLY Japanese text, line by line."
