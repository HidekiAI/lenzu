'use strict';

/**
 * renderer.js — HUD renderer
 *
 * Listens for messages / config updates pushed from the main process via the
 * context bridge (window.hudAPI) and manages the caption line queue displayed
 * in the overlay window.
 *
 * Caption line lifecycle:
 *   1. A new <div class="caption-line"> is appended to #hud-container.
 *   2. The slide-in CSS animation plays automatically.
 *   3. If maxLines is exceeded the oldest line is removed immediately.
 *   4. If displayDuration > 0 the line fades out and is removed after that
 *      many milliseconds.
 */

// ─── DOM references ───────────────────────────────────────────────────────────

const container = document.getElementById('hud-container');

// ─── State ────────────────────────────────────────────────────────────────────

/** @type {HTMLElement[]} — ordered queue, index 0 = oldest */
let lineQueue = [];

/** Runtime config — initialised from defaults then updated via IPC */
let cfg = {
  maxLines: 5,
  fontSize: 24,
  textColor: '#FFFFFF',
  backgroundColor: '#00000066',
  displayDuration: 0,
};

// ─── Config helpers ───────────────────────────────────────────────────────────

/**
 * Convert a hex colour (with optional alpha) or any CSS colour to an rgba()
 * string suitable for a CSS custom property.
 *
 * Accepts:
 *   "#RRGGBB"    → rgba(r,g,b,1)
 *   "#RRGGBBAA"  → rgba(r,g,b,a/255)
 */
function hexToRgba(hex) {
  if (!hex || !hex.startsWith('#')) return hex;
  const h = hex.slice(1);
  if (h.length === 6) {
    const r = parseInt(h.slice(0, 2), 16);
    const g = parseInt(h.slice(2, 4), 16);
    const b = parseInt(h.slice(4, 6), 16);
    return `rgba(${r},${g},${b},1)`;
  }
  if (h.length === 8) {
    const r = parseInt(h.slice(0, 2), 16);
    const g = parseInt(h.slice(2, 4), 16);
    const b = parseInt(h.slice(4, 6), 16);
    const a = parseInt(h.slice(6, 8), 16) / 255;
    return `rgba(${r},${g},${b},${a.toFixed(2)})`;
  }
  return hex;
}

/** Apply a full or partial config object to the DOM. */
function applyConfig(newCfg) {
  cfg = Object.assign(cfg, newCfg);

  // CSS custom properties drive visual appearance.
  const root = document.documentElement;
  root.style.setProperty('--hud-font-size',   `${cfg.fontSize}px`);
  root.style.setProperty('--hud-text-color',  cfg.textColor);
  root.style.setProperty('--hud-bg',          hexToRgba(cfg.backgroundColor));
}

// ─── Caption line management ──────────────────────────────────────────────────

/** Remove the oldest line from the queue and the DOM. */
function removeOldestLine() {
  if (lineQueue.length === 0) return;
  const oldest = lineQueue.shift();
  if (oldest && oldest.parentNode) {
    oldest.parentNode.removeChild(oldest);
  }
}

/**
 * Gracefully fade out and remove a specific line element.
 * @param {HTMLElement} el
 */
function fadeAndRemove(el) {
  el.classList.add('fading');
  el.addEventListener('animationend', () => {
    if (el.parentNode) el.parentNode.removeChild(el);
    const idx = lineQueue.indexOf(el);
    if (idx !== -1) lineQueue.splice(idx, 1);
    // Hide container when no lines remain.
    if (lineQueue.length === 0) container.style.visibility = 'hidden';
  }, { once: true });
}

/**
 * Add a new caption line to the HUD.
 * @param {string} text
 */
function addLine(text) {
  // Show container on first message.
  container.style.visibility = 'visible';

  const el = document.createElement('div');
  el.className = 'caption-line';
  el.textContent = text;
  container.appendChild(el);
  lineQueue.push(el);

  // Enforce max line count (immediate removal, no fade).
  let safeMaxLines = Number.parseInt(cfg.maxLines, 10);
  if (!Number.isFinite(safeMaxLines) || safeMaxLines < 0) {
    safeMaxLines = 0;
  }
  while (lineQueue.length > safeMaxLines) {
    removeOldestLine();
  }

  // Auto-remove after displayDuration ms if configured.
  if (cfg.displayDuration > 0) {
    setTimeout(() => {
      if (lineQueue.includes(el)) fadeAndRemove(el);
    }, cfg.displayDuration);
  }
}

/** Clear all displayed lines. */
function clearLines() {
  while (lineQueue.length > 0) removeOldestLine();
  container.style.visibility = 'hidden';
}

// ─── Wire up IPC callbacks ────────────────────────────────────────────────────

window.hudAPI.onMessage((text) => {
  addLine(text);
});

window.hudAPI.onConfig((newCfg) => {
  applyConfig(newCfg);
});

window.hudAPI.onClear(() => {
  clearLines();
});

// Request initial config as soon as the renderer is ready.
window.hudAPI.getConfig().then((initialCfg) => {
  applyConfig(initialCfg);
});
