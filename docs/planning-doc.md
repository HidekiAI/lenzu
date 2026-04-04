# HUD Rendering Issue Investigation

## Overview

- **lenzu_server** is broken; HUD is not rendering correctly.
- **electron-translucent-desktop-overlay** works as expected.

## Findings

- Both projects share a similar structure: TypeScript main process, preload, and renderer.
- The **renderer** HTML, CSS, and JavaScript are virtually identical between the two projects.
- The **configuration file** (`hud_config.json`) differs significantly:
    - `electron-translucent-desktop-overlay/hud_config.json` uses the keys expected by the TypeScript code (`background_opacity`, `text_color`, `font_size_pt`, `min_font_size_pt`, `default_text`, `udp_port`, `bottom_margin`).
    - `lenzu_server/hud_config.json` contains a different schema (`udpPort`, `udpBindAddress`, `position`, `opacity`, `fontSize`, `maxLines`, `textColor`, `backgroundColor`, etc.).
- In `lenzu_server/src/main.ts` the config is loaded with `loadConfig` and the code accesses properties such as `config.height`, `config.bottom_margin`, and `config.udp_port`. Because the JSON keys do not match, these values are `undefined`.
- The renderer (`src/renderer/app.ts`) expects a `HudConfig` shape matching the working overlay (e.g., `background_opacity`, `text_color`, `font_size_pt`, `default_text`). The mismatched config leads to default values being used or errors, preventing the HUD from displaying.
- No runtime errors are logged, but the window is created with a height of `undefined` and the background opacity is not applied, resulting in an invisible or incorrectly sized overlay.

## Root Cause

The **configuration schema mismatch** between `lenzu_server/hud_config.json` and the code expectations causes the HUD to receive incorrect or missing settings, which prevents proper rendering.

## Action Items

1. **Align `hud_config.json` schema** in `lenzu_server` with the expected fields:
    ```json
    {
        "height": 200,
        "background_opacity": 0.45,
        "text_color": "#f5e642",
        "font_size_pt": 24,
        "min_font_size_pt": 0,
        "default_text": "Hello world, Hello Shiroe!",
        "udp_port": 7331,
        "bottom_margin": 50
    }
    ```
2. Update any code that still references the old keys (e.g., `udpPort`, `opacity`) to use the new ones.
3. Run a test UDP message to verify that the HUD appears.
4. If additional customizations are needed (positioning, max lines), extend the `HudConfig` type and UI accordingly.

## Next Steps

- Modify `lenzu_server/hud_config.json` to match the working schema.
- Adjust any TypeScript interfaces if new fields are introduced.
- Verify HUD rendering by sending a UDP packet.
