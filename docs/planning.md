# Lenzu Project Planning & Roadmap

## Current Status
- **Platform**: Cross-platform (Windows primary, Linux secondary)
- **OCR Engines**: Windows Media OCR (fast, accurate), Tesseract (slow, less accurate)
- **UI**: GTK4 with winit/gdk4 backends
- **Capture**: WinAPI (Windows), X11 (Linux), Wayland support incomplete
- **Interpreter**: Kakasi (external binary or limited crate)

## Vision
Make **Linux the primary platform** with a robust, performant OCR lens that works across Wayland and X11, using open-source tools while maintaining the offline-first, privacy-respecting design.

## Goals

### Short-term (Next 2-4 Weeks)
1. **Modern Detection Pipeline (Phase II)**
   - Implement **Text Detection** as a standalone stage using **YOLOv8-tiny** or **DBNet** via ONNX Runtime.
   - Extract text-rectangles from full-screen/lens captures before passing to recognition.
   - Optimize detection for manga speech bubbles and vertical text layouts.

2. **Linux Capture Pipeline**
   - Complete Wayland support via gdk4-wayland (portal integration).
   - Optimize X11 capture (already implemented via x11rb).

3. **Recognition Engine Pivot**
   - Evaluate **PaddleOCR (ONNX)** or **Manga-OCR** as the primary recognition engine for extracted regions.
   - Keep Tesseract only as a lightweight fallback for clean, horizontal text.

3. **UI/UX Improvements**
   - Replace winit with pure GTK4 (remove winit dependency)
   - Add configuration UI for OCR language, PSM, and preprocessing
   - Implement lens size/position persistence
   - Add keyboard shortcuts reference overlay

4. **Packaging & Distribution**
   - Create Flatpak for easy Linux installation
   - Include jpn_vert.traineddata in package
   - Bundle kakasi as fallback (static or dynamic)
   - Add AppStream metadata for software centers

### Medium-term (1-3 Months)
1. **OCR Engine Diversification**
   - Evaluate EasyOCR (Python) via subprocess for Linux (does NOT handle vertical, discard)
   - Research manga-ocr integration (requires Python/PyTorch, heavy)
   - Investigate `rust-ocr` or `paddleocr-rs` alternatives
   - Consider training custom Tesseract model with manga109s dataset

2. **Advanced Features**
   - Text direction detection (vertical/horizontal auto)
   - Multi-line reordering for vertical text (right-to-left → left-to-right)
   - Dictionary lookup (jmdict, etc.)
   - Copy-to-clipboard with furigana toggle
   - History of recognized text

3. **Multi-monitor & HiDPI**
   - Detect all monitors and allow lens to span
   - Handle scaling factors correctly on HiDPI displays
   - Per-monitor DPI awareness

4. **Wayland Security**
   - Work with portals (xdg-desktop-portal) for screen capture
   - Fallback to XWayland compatibility mode if needed
   - Request permission for screen capture gracefully

### Long-term (3-6+ Months)
1. **Alternative OCR Backends**
   - Google Cloud Vision (online, paid) via OAuth2
   - Microsoft Azure Computer Vision (online, paid)
   - Apple Vision framework (macOS port)

2. **Machine Learning Integration**
   - On-device manga-specific model (tiny, fast)
   - Text bubble detection with YOLO/ONNX Runtime
   - Preprocessing CNN for denoising and binarization

3. **Accessibility Features**
   - Screen reader integration (Orca, speech-dispatcher)
   - High contrast mode
   - Configurable font sizes and colors

4. **Community & Ecosystem**
   - Plugin system for interpreters (kakasi, mecab, kuromoji)
   - Shared traineddata repository
   - Translation backends (DeepL, Google Translate, offline dictionaries)

## Known Issues & Blockers

### Linux Capture
- Wayland: Screen capture requires user consent via portal; may be slow
- X11: Root window capture includes all windows; need to filter by Z-order
- Multi-monitor: `xinerama` vs `randr` - need unified API
- Performance: Full-screen captures at 60fps are expensive; need region-of-interest optimization

### Tesseract Limitations
- Speed: ~5s per image with PSM 5 on manga panels
- Accuracy: ~60-70% on vertical text without preprocessing
- Training: Requires building custom traineddata with manga fonts
- Language: jpn_vert.traineddata may not include modern manga fonts

### Kakasi Integration
- Current crate is fake/limited; needs proper FFI to libkakasi
- Windows builds require manual MinGW compilation
- Linux: package manager installs kakasi CLI, but no Rust crate yet
- MeCab alternative is heavier but more accurate

## Technical Decisions

### GTK4 vs Winit
- **Decision**: Migrate to pure GTK4 (already in progress in Cargo.toml)
- **Rationale**: GTK4 provides cross-platform windowing, input, and rendering; simplifies build and dependencies

### OCR Backend Selection
- **Linux**: Tesseract CLI (via `rusty-tesseract` or custom wrapper)
- **Fallback**: If Tesseract fails, try OCR with different PSM or grayscale vs color
- **Future**: Allow user to switch between engines (Tesseract, manga-ocr, gcloud)

### Image Preprocessing Pipeline
```
Capture → Grayscale → Denoise (median filter) → Contrast stretch → Binarize (adaptive) → OCR
```
- Use `imageproc` for filters
- Benchmark each step to ensure <100ms total overhead

### Packaging
- **Primary**: Flatpak (Sandboxed, includes all dependencies)
- **Secondary**: Debian/Ubuntu PPA
- **Tertiary**: AppImage for universal Linux distribution

## Dependencies & Tools

### Must-Have (Linux)
- `tesseract-ocr` (>=5.0) with `jpn_vert` traineddata
- `kakasi` (or libkakasi-dev for FFI)
- `gtk4` (>=4.14)
- `gdk4-wayland` / `gdk4-x11`
- `pkg-config` and build-essential

### Nice-to-Have
- `opencv` (for advanced preprocessing) - but heavy dependency
- `leptonica` (Tesseract dependency, usually bundled)
- `mecab` (alternative interpreter)
- `jmdict` (dictionary lookup)

## Research Tasks

1. **Tesseract Training**: Investigate manga109s dataset format and training pipeline
2. **Wayland Portals**: Study `xdg-desktop-portal` API for screen capture permissions
3. **GTK4 Layered Windows**: Check if GTK4 supports transparent overlay windows on Wayland/X11
4. **Performance Profiling**: Profile capture → OCR pipeline to identify bottlenecks

## Milestones

- [ ] **M1**: Linux capture works on X11 with correct multi-monitor support
- [ ] **M2**: Tesseract OCR accuracy >80% on test manga pages with preprocessing
- [ ] **M3**: GTK4 UI fully functional without winit; lens window overlays correctly
- [ ] **M4**: Flatpak builds and installs on Ubuntu/Debian/Fedora
- [ ] **M5**: Kakasi integration via libkakasi FFI (not just CLI)
- [ ] **M6**: Wayland support via portals (test on GNOME/KDE)
- [ ] **M7**: Performance: Capture+OCR pipeline <2s on 1080p
- [ ] **M8**: User documentation and troubleshooting guide

## Testing Strategy

### Unit Tests
- OCR pipeline with sample images (captured from manga)
- Text reordering algorithm (vertical → horizontal)
- Image preprocessing filters (verify output quality)

### Integration Tests
- Full capture → OCR → interpreter → render flow
- Multi-monitor setup (capture from secondary monitor)
- Wayland vs X11 backend detection

### Manual QA
- Test on various manga styles (shonen, shojo, seinen)
- Different font sizes and qualities (scanned vs digital)
- Mixed vertical/horizontal text layouts

## Success Metrics

1. **Accuracy**: >90% character recognition on clean manga panels
2. **Performance**: <3s from capture to display on 1080p
3. **Usability**: Works out-of-the-box on Ubuntu 22.04+ and Fedora 38+
4. **Accessibility**: Screen reader compatible (Orca)
5. **Privacy**: No network traffic unless user enables online OCR

## References

- GTK4 Rust Book: https://gtk-rs.org/gtk4-rs/stable/latest/
- Tesseract Training: https://tesseract-ocr.github.io/tessdoc/Training-Tesseract.html
- Wayland Portals: https://flatpak.org/xdg-desktop-portal/
- Manga109: http://www.manga109.org/
- Kakasi: http://kakasi.namazu.org/

---

**Last Updated**: 2026-03-14
**Maintainer**: Hideki AI
**Status**: Planning phase - Linux-first transition

