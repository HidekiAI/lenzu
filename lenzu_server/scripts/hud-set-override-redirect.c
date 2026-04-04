/*
 * Sets override_redirect=True on the given X11 window ID (decimal or hex).
 * With override_redirect set before the window is mapped, the window manager
 * never receives a MapRequest for it and therefore never creates a decoration
 * frame — eliminating the titlebar strip that xfwm4 and similar WMs add to
 * transparent Electron windows.
 *
 * Build: gcc -O2 -o dist/hud-set-override-redirect \
 *              scripts/hud-set-override-redirect.c -lX11
 * Usage: hud-set-override-redirect <window-id>   (decimal or 0x… hex)
 */
#include <X11/Xlib.h>
#include <stdlib.h>

int main(int argc, char *argv[]) {
    if (argc < 2) return 1;
    unsigned long id = strtoul(argv[1], NULL, 0);
    Display *d = XOpenDisplay(NULL);
    if (!d) return 1;
    XSetWindowAttributes a;
    a.override_redirect = True;
    XChangeWindowAttributes(d, (Window)id, CWOverrideRedirect, &a);
    XSync(d, False);
    XCloseDisplay(d);
    return 0;
}
