# Technical Design — electron-desktop-hud

## 1. Overview

`electron-desktop-hud` renders subtitle-like text in a transparent, frameless, always-on-top window on Linux (X11). Text is delivered over UDP, so any local process can update the display without a shared library or API dependency.

---

## 2. Background & motivation

The original implementation used Tauri (Rust + WebKit2GTK). It was abandoned due to a WebKit2GTK bug: when displayed text changes, alpha pixels vacated by the old content are not cleared on the X11 surface. The compositor sees stale pixel data and blends it into the scene — "ghost text". The bug persists regardless of CSS `background: transparent` declarations.

Several mitigations were attempted in the Tauri version:

| Mitigation | Effect |
|---|---|
| CSS `body { background: rgba(10,10,10,0.02) }` (near-zero, not zero) | Forced WebKit to write pixels to the surface at all; without it the dirty-rect optimiser treated transparent→transparent as a no-op and skipped repainting entirely. |
| Body-background toggle (`BODY_BG[_bodyTick ^= 1]`) on every render | Marked the entire body layer dirty so WebKit issued a full layer repaint rather than a partial one. |
| Synthetic X11 `Expose` event after 150 ms | Forced a pixel-perfect full-window repaint at the X11 level, clearing all stale data. Equivalent to what Alt+Tab does. |

None of these fully eliminated the artefact under all timing conditions.

Electron uses Chromium, which correctly composites ARGB windows on X11 when a compositor is running. No repaint workarounds are required.

---

## 3. Process architecture

Electron runs three isolated JavaScript contexts, each with a distinct security boundary.

```
┌─────────────────────────────────────────────┐
│  Main process  (Node.js — full system access)│
│                                             │
│  • Reads hud_config.json                   │
│  • Creates BrowserWindow                   │
│  • Binds UDP socket (dgram)                │
│  • Handles IPC: get-config, move-window    │
└──────────────┬──────────────────────────────┘
               │ contextBridge (preload.js)
               │ — only named functions cross the boundary
┌──────────────▼──────────────────────────────┐
│  Renderer process  (Chromium — no Node.js)  │
│                                             │
│  • Calls window.electronHUD.getConfig()    │
│  • Listens via window.electronHUD.         │
│      onTextChanged(cb)                     │
│  • Calls window.electronHUD.moveWindow()   │
│  • Updates DOM: #subtitle-text, pill BG    │
└─────────────────────────────────────────────┘
```

`contextIsolation: true` and `nodeIntegration: false` are mandatory. They prevent renderer-side code (and any injected third-party script) from accessing Node.js APIs directly.

---

## 4. Data flow

```
External process
  │
  │  UDP datagram → 127.0.0.1:{udp_port}
  ▼
dgram.Socket.on('message')              [main process — src/main.ts]
  │
  │  text = msg.toString().trim()
  │  mainWindow.webContents.send('hud-text-changed', text)
  ▼
ipcRenderer.on('hud-text-changed', …)   [preload — src/preload.ts]
  │
  │  callback(text)
  ▼
renderText(text)                        [renderer — src/renderer/app.ts]
  │
  ├─ subtitleText.textContent = text
  ├─ subtitleText.style.color = cfg.text_color
  ├─ subtitleText.style.fontSize = cfg.font_size_pt + 'pt'
  └─ subtitleBox.style.background = text ? pill color : 'transparent'
```

**Window repositioning (arrow keys):**

```
keydown ArrowUp/ArrowDown               [renderer]
  │
  │  window.electronHUD.moveWindow(position)
  ▼
ipcRenderer.send('move-window', position)
  ▼
ipcMain.on('move-window', …)            [main process]
  │
  │  computePosition(display.workArea, winSize, position, margin)
  └─ mainWindow.setPosition(x, y)
```

---

## 5. Window configuration

| Property | Value | Reason |
|---|---|---|
| `transparent: true` | — | Enables ARGB visual on X11 |
| `frame: false` | — | No title bar or borders |
| `backgroundColor: '#00000000'` | — | Fully transparent initial paint; avoids a white flash before the renderer loads |
| `alwaysOnTop: true` | — | Stays above other windows |
| `skipTaskbar: true` | — | Excluded from taskbar / alt-tab list |
| `resizable: false` | — | Fixed layout; prevents accidental resize |
| `hasShadow: false` | — | No drop shadow on a transparent window |
| `width` | `primaryDisplay.workAreaSize.width` | Full screen width so the pill can be centered via CSS on any display |
| `height` | `config.height` | Configurable; default 200 px |

`app.commandLine.appendSwitch('enable-transparent-visuals')` must be called **before** `app.whenReady()`. It instructs Chromium to request an ARGB X11 visual from the X server. Without it, the window background is opaque black even if `transparent: true` is set.

---

## 6. UDP design

- **Transport:** UDP, loopback only (`127.0.0.1`). TCP is not used because delivery order and acknowledgement are irrelevant — only the most recent text matters.
- **Payload:** Raw UTF-8 string, no framing, max 65 507 bytes (UDP datagram limit). Whitespace is trimmed. Empty payload after trimming is ignored.
- **Binding:** The socket is bound after `app.whenReady()` inside the same callback chain that creates the window. This guarantees the window exists before any message can be processed.
- **Error handling:** On socket error the socket is closed; the app continues running with the last displayed text frozen.

---

## 7. Configuration loading

`loadConfig(path)` in `src/config.ts`:

1. Reads the file synchronously (startup only — no hot reload).
2. Parses JSON and shallow-merges over `DEFAULT_CONFIG`.
3. On any error (missing file, malformed JSON, permission denied) returns `DEFAULT_CONFIG` unchanged.

The function takes a file path argument rather than constructing the path internally, which makes it straightforward to unit-test without mocking the filesystem.

---

## 8. Position calculation

`computePosition(display, windowSize, position, margin)` in `src/window-position.ts`:

```
bottom:  y = display.y + display.height − winH − margin
         x = display.x + ⌊(display.width − winW) / 2⌋

top:     y = display.y + margin
         x = (same horizontal centering)

center:  y = display.y + ⌊(display.height − winH) / 2⌋
         x = (same horizontal centering)
```

`margin` applies only to top and bottom positions. The function is pure (no I/O, no global state), takes `display.workArea` (which already excludes taskbar), and returns integer pixel coordinates.

---

## 9. Build pipeline

```
src/main.ts     ──esbuild──▶  dist/main.js      (Node, CommonJS, bundled)
src/preload.ts  ──esbuild──▶  dist/preload.js   (Node, CommonJS, bundled)
src/renderer/
  app.ts        ──esbuild──▶  dist/renderer/app.js   (browser, IIFE)
  index.html    ──cp──────▶  dist/renderer/index.html
  styles.css    ──cp──────▶  dist/renderer/styles.css
```

esbuild is invoked directly via `build.mjs` (an ES module script). There is no Vite or webpack layer. This keeps cold build time under 200 ms and avoids framework-specific quirks in a project this small.

`tsconfig.json` is used only for type checking (`tsc --noEmit`). esbuild handles transpilation independently and ignores `tsconfig.json` emit settings.

---

## 10. Testing strategy

Only pure modules are unit-tested. Electron-bound code (`main.ts`, `preload.ts`, the renderer DOM) is excluded from automated tests — it requires a real display and Electron runtime.

| Module | What is tested |
|---|---|
| `src/config.ts` | Default fallback, partial override, malformed JSON, empty file, immutability of `DEFAULT_CONFIG` |
| `src/window-position.ts` | Bottom/top/center placement, horizontal centering, offset display (second monitor), narrow window on wide display |

**Framework:** vitest (runs via Node.js, no browser environment needed).

---

## 11. Known limitations

- **X11 only.** Wayland compositors do not implement the same ARGB visual protocol. `--enable-transparent-visuals` has no effect under XWayland. The app should still launch but the background will be opaque.
- **White titlebar strip (Electron 41+ on X11).** Despite `frame: false`, Electron 41+ can show a thin white strip at the top of the window on some X11 compositors. Mitigated by setting `type: 'toolbar'` and `titleBarStyle: 'hidden'` on `BrowserWindow`, which sets the `_NET_WM_WINDOW_TYPE_TOOLBAR` hint to suppress window-manager decorations. This reduces the strip significantly but may not fully eliminate it on all compositor/WM combinations.
- **Compositor required.** Without an ARGB compositor the window background is black.
- **Single window.** Only the primary display's work area is used to size the window. The window is repositioned to whatever display the cursor is on at the time an arrow key is pressed.
- **No hot reload.** `hud_config.json` is read once at startup. The app must be restarted to pick up config changes.
- **UDP loopback only.** The socket binds `127.0.0.1` by design. Remote senders are not supported.
