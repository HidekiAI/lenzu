# lenzu_client (DEPRECATED — Tauri/WebKit2GTK overlay)

> **This directory is deprecated and no longer used.**
>
> The Tauri + WebKit2GTK overlay was abandoned because WebKit2GTK does not correctly composite ARGB windows on X11: alpha pixels vacated by old content are not cleared on the X11 surface, causing "ghost text" accumulation on every text update. Multiple workarounds (near-zero body background, body-background toggle, synthetic X11 Expose events) were attempted but none fully eliminated the artefact under all timing conditions.
>
> The active overlay is [`../lenzu_server`](../lenzu_server), which uses **Electron (Chromium)**. Chromium correctly composites ARGB windows when a compositor is running and requires no repaint workarounds.
>
> This directory is kept for historical reference only. Do not use it.

## Historical reference only

> The remainder of this document is preserved solely as archival reference for the deprecated Tauri/WebKit2GTK overlay.
> Do **not** use the configuration, compositor, or build steps below for current development or deployment.
> Use [`../lenzu_server`](../lenzu_server) instead.

## Configuration (`hud_config.json`)

Optional file in this directory. All fields have defaults.

```json
{
  "height": 200,
  "background_opacity": 0.72,
  "text_color": "#f5e642",
  "font_size_pt": 24,
  "min_font_size_pt": 0,
  "bottom_margin": 50,
  "udp_port": 7331,
  "default_text": "Lenzu HUD ready."
}
```

`udp_port` here is only used when `--port` is **not** passed on the command line. When launched by `lenzu`, the port always comes from `--port`.

## X11 compositor: xfwm4 vs picom

### Symptom
Window background appears opaque/dark instead of transparent, or ghost pixels accumulate as text changes.

### Root cause
xfwm4's built-in compositor uses alpha-blend-over accumulation — each semi-transparent frame is blended *on top of the previous buffer* rather than composited fresh against the desktop. Semi-transparent areas fill up with dark junk over time.

### Diagnosis
```bash
ps auxf | grep -E "picom|compton|xfwm"
echo $XDG_SESSION_TYPE   # must be "x11", not "wayland"
```
If you see `xfwm4` and no `picom`, that's the problem.

### Fix
```bash
# 1. Install picom
sudo apt install -y picom

# 2. Disable xfwm4's compositor (via CLI or Settings → Window Manager Tweaks → Compositor)
xfconf-query -c xfwm4 -p /general/use_compositing -s false

# 3. Start picom — --no-use-damage forces full redraws, preventing ghost pixels
picom --backend glx --no-use-damage &

# 4. Restart lenzu_server so its window registers under the new compositor
```

If GLX is unavailable, substitute `--backend xrender`.

**Make it permanent:** XFCE → Session and Startup → Application Autostart → add `picom --backend glx --no-use-damage`.

## Building for production

```bash
npm run tauri build
```

Output binary: `src-tauri/target/release/lenzu_server`
