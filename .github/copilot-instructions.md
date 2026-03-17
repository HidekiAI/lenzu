# Copilot instructions for contributors and AI agents

This project is a Rust desktop OCR/magnifier focused on Japanese vertical/horizontal text. The goal of this file is to give AI coding agents enough context to be productive quickly.

Keep answers short and actionable. When proposing code changes, include exact file paths (e.g. `lenzu/src/main.rs`) and minimal patches. Prefer small, incremental edits and run `cargo build` locally to verify changes.

Key concepts

-   Architecture: workspace with primary crate `lenzu` and several prototypes under `prototypes/`.

    -   `lenzu/` is the main application (desktop magnifier + OCR). Primary entrypoint: `lenzu/src/main.rs`.
    -   `prototypes/` contains experimental crates (kakasi CLI test, static libs, windows-rs OCR examples).
    -   `assets/` contains OCR assets (traineddata like `jpn_vert.traineddata`, dictionaries, fonts).

-   Major components (see `lenzu/src/`):
    -   `ocr_traits.rs` — trait(s) for OCR backends.
    -   `ocr_tesseract.rs` — Tesseract-based OCR implementation (used on non-Windows or when forced).
    -   `ocr_winmedia.rs` — Windows Media.Ocr integration (preferred on Windows).
    -   `ocr_gcloud.rs` — optional Google Cloud Vision integration (online alternative).
    -   `interpreter_traits.rs` / `interpreter_ja.rs` — post-processing (kakasi conversion to hiragana).
    -   `image_handling.rs` — helpers for image capture, grayscale, overlay, and mapping between screen and window.
    -   `cursor_data.rs` — cursor + capture rectangle handling.

Why things are structured this way

-   Platform separation: Windows-specific features (Media_Ocr, windows-rs, winapi) live behind cfg(windows) and are preferred on Windows for accuracy. Tesseract is cross-platform but often lower accuracy for vertical Japanese; it's the non-Windows default.
-   Trait-based backends: OCR and interpreter logic are abstracted via traits so adding/removing OCR engines or post-processors is isolated.
-   CLI / external binary usage: some behavior relies on calling external programs (kakasi, tesseract). Expect code to shell out or use crates that call executables (e.g. `rusty-tesseract`).

Build & developer workflows

-   Build (Linux/macOS): use cargo from a normal POSIX shell.
    -   `cargo build --workspace` to build all workspace members.
    -   For the `lenzu` binary: `cargo build -p lenzu` or `cargo run -p lenzu -- <args>`.
-   Windows: use a proper MinGW64 or MSVC environment depending on linking needs. When using MinGW toolchain for dependencies like `rusty-tesseract` or `kakasi` static libs, build on MinGW64 shell (pacman environment) to avoid linker pain.
-   Native dependencies: install OS-level packages when required:
    -   Tesseract + traineddata (e.g. `jpn_vert.traineddata`) and leptonica for Tesseract crates.
    -   `kakasi` executable when invoking kakasi externally.
-   Debugging image capture (Windows): `lenzu/src/main.rs` contains detailed screen-capture and window layering logic. If capture shows mirror/missing bits, check `hide_window()`/`show_window()` usage and layered window attributes.

Project-specific conventions and patterns

-   Trait usage: prefer adding new OCR or interpreter backends by implementing the existing traits (`OcrTrait`, `InterpreterTrait`) and returning boxed trait objects via factory functions `create_ocr()` / `create_interpreter()` found in `main.rs`.
-   Platform selection: `create_ocr()` picks backend via `cfg!(target_os = "windows")` and args. Preserve that behavior unless explicitly changing cross-platform defaults.
-   Image flow in `main.rs`:
    1. hide window (set layered alpha nearly transparent)
    2. capture screen into bitmap via Win32 APIs
    3. show window
    4. grayscale + OCR evaluate
    5. run interpreter (kakasi) to convert kanji -> hiragana
    6. overlay text image and BitBlt back into the window
    -   See `capture_and_ocr()` and `from_screen_to_image()` for exact steps and ordering.
-   Debug artifacts: code conditionally writes `recognized_image.png` when built in debug mode. Use that for offline inspection.

Integration points & external dependencies

-   Tesseract: `rusty-tesseract` crate is used; it often shells out and wants OS-level tesseract installed. See `Cargo.toml` top-level and `lenzu/Cargo.toml` for versions.
-   Windows Media OCR: enabled by `windows` crate features in `lenzu/Cargo.toml` and by building on Windows with `Media_Ocr` feature.
-   Kakasi: external executable or crate; historically code expects the kakasi CLI (or prototype crate). If adding support, check `interpreter_ja.rs` and how the InterpreterTrait returns `InterpreterTraitResult`.
-   Google Cloud Vision: `ocr_gcloud.rs` exists as an alternative. Expect OAuth/credentials if enabling online mode.

Examples to reference (search these files for concrete patterns)

-   `lenzu/src/main.rs` — full capture -> ocr -> render flow; implement new features here carefully.
-   `lenzu/src/ocr_tesseract.rs` — example of calling Tesseract and mapping results into `OcrResult` structures.
-   `lenzu/src/ocr_winmedia.rs` — example of Windows-specific API usage and `windows` crate feature flags.
-   `lenzu/Cargo.toml` and top-level `Cargo.toml` — workspace members and platform-specific dependencies.
-   `README.md` — high-level design decisions and rationales (offline-first, prefer Windows Media.Ocr when available).

What AI agents should do first when changing code

-   Run `cargo build -p lenzu` locally and fix compile errors incrementally. Keep diffs small.
-   If adding a backend, copy patterns from `ocr_tesseract.rs` / `ocr_winmedia.rs` and keep trait contracts intact.
-   If modifying capture or window layering behavior, test on Windows where that code runs. Use debug image outputs (`recognized_image.png`) to validate image pipeline.
-   When changing Cargo features (e.g. windows features), update `lenzu/Cargo.toml` and run `cargo build` on the matching platform.

Safety and performance notes

-   Image capture uses raw Win32 APIs and manual memory handling. Be conservative with unsafe blocks; preserve existing resource cleanup (DeleteDC, ReleaseDC, DeleteObject).
-   OCR can be slow when multiple languages are requested. `lenzu` passes a `supported_lang` string; longer lists increase OCR time linearly.

Quick checklist for common tasks

-   Add new OCR backend: implement `OcrTrait` in `lenzu/src/`, add factory path in `create_ocr()` in `main.rs`, update `Cargo.toml` if new crate is added.
-   Fix capture mirroring: inspect `hide_window()` / `show_window()` and `SetLayeredWindowAttributes` calls in `main.rs`.
-   Make Tesseract more reliable: preprocess images in `image_handling.rs` (partition panels, grayscale, denoise) before calling `evaluate()`.

If anything here is unclear or you want a different level of detail (more examples, file snippets, or CI/build commands), tell me which area to expand and I'll iterate.
