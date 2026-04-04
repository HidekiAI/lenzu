# electron-translucent-desktop-overlay

A **transparent, HUD-like desktop overlay** for Linux that displays text
messages received over **UDP** as closed-caption / subtitle lines.  The
overlay is click-through (does not obstruct any other application), always
on top, and fully configurable for position, translucency and font size.

> Replaces the deprecated
> [tauri-translucent-desktop-overlay](https://github.com/HidekiAI/tauri-translucent-desktop-overlay)
> (Tauri + WebKit2GTK). WebKit2GTK does not correctly composite ARGB windows on
> X11 — stale alpha pixels accumulate as "ghost text" on every update, and no
> workaround fully eliminates it. Electron (Chromium) composites ARGB correctly.
>
> **Known limitation (Electron 41+ / X11):** A thin white titlebar strip may
> appear at the top of the window despite `frame: false`. Mitigated with
> `type: 'toolbar'` + `titleBarStyle: 'hidden'` but not fully eliminated on all
> compositor/WM combinations.

![simplescreenrecorder-2026-03-23_18 57 28](https://github.com/user-attachments/assets/b65d6be5-2592-48b9-858d-998f8c873cd8)

---

## Features

| Feature | Detail |
|---|---|
| Transparent / translucent window | `opacity` config (0.0 – 1.0) |
| Click-through | Will not intercept mouse events |
| Position control | `bottom-center`, `top-center`, or custom `x`/`y` |
| Font size | `fontSize` config (pixels) |
| UDP input | Default port **5005** — plain text or JSON |
| Runtime reconfiguration | Send a JSON `config` command over UDP |
| Max visible lines | Oldest lines scroll off automatically |
| Auto-dismiss | Optional `displayDuration` (ms) per line |

---

## Prerequisites

- **Node.js** ≥ 18
- A compositing window manager on Linux (e.g. GNOME, KDE Plasma, i3+picom)
  so that the transparent Electron window is actually composited.

---

## Installation

```bash
npm install
```

---

## Running

### Standard (Wayland / GNOME / KDE)

```bash
npm start
```

### X11 with compositing (e.g. i3 + picom)

On X11 you must pass the `--enable-transparent-visuals` flag so Chromium
picks up a 32-bit visual from the compositor:

```bash
npm run start:x11
```

---

## Sending messages

Any UDP client can send text to port **5005** (localhost by default).

### Plain text

```bash
echo -n "Hello, World!" | nc -u -w1 127.0.0.1 5005
```

### JSON protocol

All JSON messages must be sent as a single UTF-8 UDP datagram.

#### Display a message

```json
{ "type": "message", "text": "Caption text here" }
```

#### Update config at runtime

```json
{
  "type": "config",
  "settings": {
    "fontSize": 32,
    "opacity": 0.7,
    "position": "top-center",
    "textColor": "#FFFF00",
    "backgroundColor": "#00000099",
    "maxLines": 3,
    "displayDuration": 4000
  }
}
```

#### Clear all lines

```json
{ "type": "clear" }
```

#### Quit the overlay

```json
{ "type": "quit" }
```

---

## Configuration (`src/config.json`)

Edit `src/config.json` to change the defaults before launch:

| Key | Type | Default | Description |
|---|---|---|---|
| `udpPort` | number | `5005` | UDP port to listen on |
| `udpBindAddress` | string | `"127.0.0.1"` | Address to bind the UDP server to |
| `position` | string | `"bottom-center"` | `"bottom-center"` \| `"top-center"` \| `"custom"` |
| `x` | number\|null | `null` | Window X when `position="custom"` |
| `y` | number\|null | `null` | Window Y when `position="custom"` |
| `opacity` | number | `0.85` | Window opacity (0.0 – 1.0) |
| `fontSize` | number | `24` | Caption font size in px |
| `maxLines` | number | `5` | Maximum visible caption lines |
| `textColor` | string | `"#FFFFFF"` | CSS colour for caption text |
| `backgroundColor` | string | `"#00000066"` | CSS colour for caption background |
| `width` | number | `800` | Window width in px |
| `height` | number | `200` | Window height in px |
| `displayDuration` | number | `0` | Auto-dismiss each line after N ms (`0` = never) |

---

## Test sender

A helper script sends a sequence of demo messages and runtime config changes:

```bash
node test_sender.js [port] [host]
# e.g.
node test_sender.js 5005 127.0.0.1
```

---

## Architecture

```
┌─────────────────────────────────────────────┐
│  Electron main process  (src/main.js)       │
│  ┌──────────┐   IPC (hud:message)           │
│  │UDP server│ ──────────────────────────►   │
│  │ dgram    │   IPC (hud:config)            │
│  │ :5005    │ ──────────────────────────►   │
│  └──────────┘   IPC (hud:clear)             │
│                                             │
│  BrowserWindow — transparent, frameless,   │
│  always-on-top, click-through              │
│       │  (preload.js / contextBridge)       │
│       ▼                                     │
│  Renderer (src/renderer.js + index.html)   │
│  Caption line queue displayed as subtitles │
└─────────────────────────────────────────────┘
```
