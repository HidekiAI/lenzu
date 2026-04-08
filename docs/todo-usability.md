# Usability TODO

Three usability issues to address after local ollama is working.

---

## 1. Cursor replacement when focus is lost (P1)

**Desired behavior:** When the user clicks into another app (e.g. a browser window to scroll/pan the page they're reading), the lens window should continue to act as the visible cursor — i.e. the system cursor should be hidden globally so the lens is the only cursor visual. When Alt-Tab returns focus to lenzu, same behavior is maintained seamlessly.

**Current behavior:** The lens window tracks the cursor position every 16ms via `root_win.device_position()` and always stays `keep_above`. But the default system cursor is still visible in other apps, so there are two "cursors" on screen.

**Why it "works" on a different desktop already:** When lenzu is on a different X11 desktop (workspace), its GTK window is invisible and doesn't compete with the current cursor. The user has noted they like that feel.

### Technical approach

Use **XFixes** (`XFixesHideCursor` / `XFixesShowCursor`) to globally hide the system cursor while lenzu is running. The lens window (already tracking cursor position every 16ms) becomes the visual cursor.

`x11rb` (already in `Cargo.toml` with `all-extensions`) has XFixes support:

```rust
use x11rb::connection::Connection;
use x11rb::protocol::xfixes::{self, ConnectionExt as _};

fn hide_cursor(conn: &impl Connection, root: u32) {
    xfixes::hide_cursor(conn, root).unwrap();
    conn.flush().unwrap();
}

fn show_cursor(conn: &impl Connection, root: u32) {
    xfixes::show_cursor(conn, root).unwrap();
    conn.flush().unwrap();
}
```

The `x11rb` connection can be opened in `main.rs` alongside the existing GTK init. Store it in `AppState` or as a top-level `Rc`. Call `hide_cursor` once at startup; call `show_cursor` in the GTK `delete_event` / cleanup path.

**Edge case:** During a Shift+Click capture the lens window hides briefly (`window_main.hide()`). During that hide, temporarily call `show_cursor` so the user isn't left cursor-less, then `hide_cursor` again when the window returns.

```rust
// before capture:
window_main.hide();
show_cursor(&conn, root);

// after capture results arrive (in the rx handler where is_loading → false):
hide_cursor(&conn, root);
window_main.show();
```

**Cleanup on panic/exit:** Register a `ctrlc` handler (or use `std::panic::set_hook`) that calls `show_cursor` before exiting, otherwise the system cursor stays hidden after lenzu crashes.

---

## 2. HUD visible on all workspaces (P1)

**Desired behavior:** The Electron HUD should appear on every virtual desktop / workspace the user switches to (`Ctrl+Alt+Arrow`). This already works due to `override_redirect=True`, but it should be made explicit and robust.

**Why it currently works:** `override_redirect=True` (set via the C helper in `lenzu_server/scripts/hud-set-override-redirect.c`) bypasses the window manager entirely — the WM doesn't track, manage, or assign the window to any workspace. The X11 compositor draws it wherever the X server says, which is "everywhere."

**Risk of breakage:** If the override-redirect helper ever fails (e.g. the binary isn't found), Electron's `alwaysOnTop: true` is the only safety net, and some WMs will confine the window to the current workspace.

### Technical approach — add Electron's built-in sticky API as belt-and-suspenders

In `lenzu_server/src/main.ts`, after the `mainWindow = new BrowserWindow(...)` block and before `mainWindow.loadFile(...)`:

```typescript
// Make the HUD visible on all workspaces/virtual desktops.
// This is belt-and-suspenders alongside override_redirect — if the
// C helper ever fails, this ensures the WM still marks the window sticky.
mainWindow.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true });
```

`setVisibleOnAllWorkspaces` sets `_NET_WM_DESKTOP = 0xFFFFFFFF` and `_NET_WM_STATE_STICKY` on Linux X11, which is the EWMH-standard way to pin a window to all desktops. Combined with `override_redirect`, this is doubly guaranteed.

**Note:** `focusable: false` must remain set (it already is) to prevent the HUD from stealing keyboard focus when workspace-switching.

---

## 4. Hide HUD overlay before screen capture (P1)

**Problem:** The Electron HUD overlay (`lenzu_server`) is visible on screen when Shift+Click or Ctrl+Shift+Click triggers a capture. The X11 root-window `GetImage` call captures all on-screen pixels, so any text currently displayed in the HUD will appear in the captured image and be re-OCR'd — polluting the translation results with the previous output.

**Fix:** Before calling `capture_x11`, send a UDP message to `lenzu_server` to hide the overlay content, wait at least one compositor frame (≥16 ms) for the screen to update, then capture, then restore the HUD.

**Implementation sketch (main.rs):**

```rust
// Existing flow (around line 539–563):
window_main.queue_draw();
window_main.hide();
while gtk::events_pending() { gtk::main_iteration(); }
// ← add: send UDP {"type":"hide"} to lenzu_server here
std::thread::sleep(Duration::from_millis(400));  // compositor settle — HUD clear within this window
let raw = capture::capture_x11(cap_x, cap_y, cap_w, cap_h);
// ← after capture results arrive in rx handler: send UDP {"type":"show"} to restore HUD
```

**IPC change (lenzu_server):** Add a `"hide"` / `"show"` message type to `renderer.js`:
```js
// renderer.js
ipcRenderer.on('hud-visibility', (_, msg) => {
  document.body.style.visibility = msg === 'hide' ? 'hidden' : 'visible';
});
```
and in `main.js` forward the UDP JSON to the renderer when `type === 'hide'` or `'show'`.

---

## 3. Spinner "line" artifact on Shift+Click (P3 — cosmetic)

**Desired behavior:** The loading spinner should be a clean rotating arc with no stray line.

**Current behavior:** A faint line is drawn from the center (or previous path point) to the start of the arc. This is a Cairo artifact: `cr.arc()` implicitly draws a straight line from the current path position to the arc's start point before drawing the arc.

**Root cause:** In `lenzu/src/main.rs:322`, after `cr.rotate(s.spinner_angle)`, the Cairo context has no current point set (the path is empty after `save/restore` from the previous draw), but Cairo's `arc()` call will add a line-to from wherever the pen currently is. On first call this may be (0,0), producing a line from center to the arc circumference.

```rust
// Current (main.rs:313–325):
cr.arc(0.0, 0.0, 8.0, 0.0, 1.5 * std::f64::consts::PI);

// Fix: call new_sub_path() first so Cairo starts a fresh subpath at the
// arc start without drawing a connecting line:
cr.new_sub_path();
cr.arc(0.0, 0.0, 8.0, 0.0, 1.5 * std::f64::consts::PI);
```

One-line fix in `lenzu/src/main.rs` around line 322.
