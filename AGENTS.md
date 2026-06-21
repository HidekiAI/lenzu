# Shared Knowledge Conventions

## Cross-repo references

- **Main lenzu repo (production):** `~/projects/remote/github/mine/codemonkeyninja/lenzu`
- **This repo (prototypes):** `/usr/src/github/mine/hidekiai/lenzu-prototypes`

## Shared discovery documents

Both repos maintain separate discovery docs in the main repo's `docs/` dir
to avoid two workspaces stomping each other's writes:

| File | Written from | Purpose |
|------|-------------|---------|
| `lenzu/docs/prototypes-desktop-issues.md` | **This workspace** | GTK3/4 overlay/windowing findings from prototypes |
| `lenzu/docs/lenzu-desktop-issues.md` | Main lenzu repo | GTK dep analysis from production perspective |

**Convention:** Each workspace writes ONLY to its own file. Content can
cross-reference the other file as needed.

## Key findings (memorized for this workspace)

1. **GTK3→4 migration complete** — all 4 GTK members ported to gtk4-rs 0.11.x / glib 0.22.x.

2. **GdkScreen removal** — `gdk::Screen::default().unwrap().root_window()` for global
   pointer tracking has no GTK4 GDK equivalent. Workaround: use `x11rb`
   directly for pointer position queries.

3. **GdkSurface methods not exposed in gtk4-rs 0.11** — `move_to()`,
   `input_shape_combine_region()` require x11rb replacements.

4. **GTK4-rs 0.8.x and 0.11.x cannot mix** — different glib/gdk-pixbuf/pango
   generations (0.19 vs 0.22). All members must use same generation.

5. **5 open dependabot PRs** all fail on Gemini `review / review` check (infra
   timeout). PRs #31–#34 are GTK4 ecosystem bumps (tightly coupled, now superseded);
   PR #35 (tokenizers) is independent.

6. **GTK3 0.18.2 is the final GTK3 release** — crate is marked UNMAINTAINED.

7. **Compositor ghosting issue** on xfwm4 — `picom --backend glx --no-use-damage`
   as mitigation.

8. **glib 0.22 `clone!` macro** — uses `#[strong]/#[weak]` attribute syntax,
   not `@strong/@weak`.

9. **Window management on X11** — use x11rb directly:
   - `query_pointer()` for cursor tracking
   - `configure_window()` for positioning
   - `_NET_WM_STATE` ClientMessage for keep-above
   - `gdk4_x11::X11Surface::xid()` for getting XID (returns u64, cast to u32)

10. **`X11Surface::xid()` returns u64** — must cast to `u32` for x11rb APIs.

11. **`OnceLock::get_or_try_init` is unstable** — use manual `set()` + `get()`
    pattern instead.

12. **Unit tests for RefCell borrow panics** — `RefCell` is not `UnwindSafe`,
    so `catch_unwind` calls need `std::panic::AssertUnwindSafe` wrapper.
    Tests in `jp_ocr_app` and `x11-gtk-lens-test` verify the old pattern
    (holding borrow across blocking) panics and the scoped-guard pattern
    (drop before re-borrow) does not.

13. **SCIM stderr noise** — GTK4 GDK X11 auto-launches `scim-launcher -f x11`
    which prints "Loading socket Config module... / Failed to load x11 FrontEnd
    module." to stderr on every startup. The X11 frontend module init fails
    internally. Cosmetic only — app unaffected. `GTK_IM_MODULE` has no effect.
    Suppress with `2>/dev/null` or `apt remove scim`.

## Session startup

When starting a session in this workspace:
1. Read `~/projects/remote/github/mine/codemonkeyninja/lenzu/docs/prototypes-desktop-issues.md`
   for current state
2. Optionally check `~/projects/remote/github/mine/codemonkeyninja/lenzu/docs/lenzu-desktop-issues.md`
   for the main repo's perspective
