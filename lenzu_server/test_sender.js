#!/usr/bin/env node
'use strict';

/**
 * test_sender.js — UDP test sender for the HUD overlay
 *
 * Sends a series of plain-text and JSON command messages to the running HUD
 * overlay for manual testing.
 *
 * Usage:
 *   node test_sender.js [port] [host]
 *
 * Defaults: port=5005, host=127.0.0.1
 *
 * Examples:
 *   node test_sender.js
 *   node test_sender.js 5005
 *   node test_sender.js 5005 192.168.1.100
 */

const dgram = require('dgram');

const PORT = parseInt(process.argv[2], 10) || 5005;
const HOST = process.argv[3] || '127.0.0.1';

const MAX_LOG_LENGTH = 80;

const client = dgram.createSocket('udp4');

/**
 * Send a single UDP message.
 * @param {string|object} payload  String → sent as-is; object → JSON-serialised.
 * @returns {Promise<void>}
 */
function send(payload) {
  const msg =
    typeof payload === 'string' ? payload : JSON.stringify(payload);
  return new Promise((resolve, reject) => {
    client.send(Buffer.from(msg, 'utf8'), PORT, HOST, (err) => {
      if (err) reject(err);
      else {
        console.log(`  sent → ${msg.length > MAX_LOG_LENGTH ? msg.slice(0, MAX_LOG_LENGTH) + '…' : msg}`);
        resolve();
      }
    });
  });
}

/** Sleep for `ms` milliseconds. */
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function run() {
  console.log(`\nUDP test sender  →  ${HOST}:${PORT}\n`);

  // ── 1. Plain-text caption messages ──────────────────────────────────────────
  console.log('=== Plain-text messages ===');
  await send('Hello, World!  This is the HUD overlay.');
  await sleep(1200);
  await send('The quick brown fox jumps over the lazy dog.');
  await sleep(1200);
  await send('Testing line wrapping with a much longer sentence that should wrap inside the caption box.');
  await sleep(1200);
  await send('Line 4 — four lines visible now.');
  await sleep(1200);
  await send('Line 5 — five lines, oldest should scroll off.');
  await sleep(1200);
  await send('Line 6 — line 1 should have disappeared.');
  await sleep(1500);

  // ── 2. JSON message command ──────────────────────────────────────────────────
  console.log('\n=== JSON { type:"message" } ===');
  await send({ type: 'message', text: 'JSON message command works.' });
  await sleep(1200);

  // ── 3. Runtime config update — increase font size ────────────────────────────
  console.log('\n=== Runtime config: fontSize → 32 ===');
  await send({ type: 'config', settings: { fontSize: 32 } });
  await sleep(800);
  await send('Font size is now 32px.');
  await sleep(1200);

  // ── 4. Runtime config update — change opacity ────────────────────────────────
  console.log('\n=== Runtime config: opacity → 0.5 ===');
  await send({ type: 'config', settings: { opacity: 0.5 } });
  await sleep(800);
  await send('Opacity reduced to 0.5 — more translucent now.');
  await sleep(1200);

  // ── 5. Runtime config update — restore defaults ──────────────────────────────
  console.log('\n=== Runtime config: restore defaults ===');
  await send({
    type: 'config',
    settings: { fontSize: 24, opacity: 0.85 },
  });
  await sleep(800);
  await send('Back to default font size and opacity.');
  await sleep(1200);

  // ── 6. Runtime config update — move to top ───────────────────────────────────
  console.log('\n=== Runtime config: position → top-center ===');
  await send({ type: 'config', settings: { position: 'top-center' } });
  await sleep(800);
  await send('Overlay moved to top-center of the screen.');
  await sleep(1500);

  // ── 7. Move back to bottom ───────────────────────────────────────────────────
  await send({ type: 'config', settings: { position: 'bottom-center' } });
  await sleep(800);
  await send('Back to bottom-center.');
  await sleep(1200);

  // ── 8. Clear command ─────────────────────────────────────────────────────────
  console.log('\n=== Clear command ===');
  await send({ type: 'clear' });
  console.log('  Cleared all lines.');
  await sleep(800);
  await send('Lines cleared! Fresh start.');
  await sleep(1500);

  console.log('\nDone. Closing sender.\n');
  client.close();
}

run().catch((err) => {
  console.error('Error:', err);
  client.close();
  process.exit(1);
});
