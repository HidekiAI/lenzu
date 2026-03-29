# lenzu_server

Transparent desktop overlay HUD for the [Lenzu](../lenzu) OCR lens. Receives text via UDP and renders it in a translucent Tauri window pinned to the bottom of the screen.

Part of the Lenzu workspace. In normal use `lenzu` (the client) will spawn and kill this process automatically (Phase 2). For now it must be started separately.

## Running standalone

```bash
cd lenzu_server
npm install          # first time only — installs @tauri-apps/cli
npm run tauri dev -- -- --port 7331
```

The `--port` argument overrides `hud_config.json`. Use the same port as `overlay_udp_port` in `lenzu/lenzu_config.json`.

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
