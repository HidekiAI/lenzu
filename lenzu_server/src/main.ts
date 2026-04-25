import { app, BrowserWindow, ipcMain, screen } from 'electron';
import * as path from 'path';
import * as dgram from 'dgram';
import { execSync } from 'child_process';
import { loadConfig, DEFAULT_CONFIG, type HudConfig } from './config';
import { computePosition, type WindowPosition } from './window-position';

// Electron 36–41 had a Linux X11 regression where transparent:true rendered
// as opaque white.  Re-tested on 41.3.0 (2026-04-25) and confirmed fixed.
// Keeping a soft probe here so a future regression is loud at startup.
console.info(`[HUD] Electron ${process.versions.electron} starting.`);

// Required for ARGB transparent windows on X11.  Without this flag Chromium
// requests a 24-bit visual and transparent: true has no effect.
app.commandLine.appendSwitch('enable-transparent-visuals');

let mainWindow: BrowserWindow | null = null;
let config: HudConfig = DEFAULT_CONFIG;

function positionWindow(position: WindowPosition): void {
  if (!mainWindow) return;
  const cursor = screen.getCursorScreenPoint();
  const display = screen.getDisplayNearestPoint(cursor);
  const winSize = mainWindow.getSize() as [number, number];
  const { x, y } = computePosition(display.workArea, winSize, position, config.bottom_margin);
  mainWindow.setPosition(x, y);
}

app.whenReady().then(() => {
  config = loadConfig(path.join(app.getAppPath(), 'hud_config.json'));

  const primaryDisplay = screen.getPrimaryDisplay();
  const screenWidth = primaryDisplay.workAreaSize.width;

  mainWindow = new BrowserWindow({
    width: screenWidth,
    height: config.height,
    transparent: true,
    frame: false,
    show: false,
    alwaysOnTop: true,
    skipTaskbar: true,
    resizable: false,
    hasShadow: false,
    focusable: false,
    backgroundColor: '#00000000',
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
    },
  });

  const buf = mainWindow.getNativeWindowHandle();
  const winIdNum = buf.length >= 8
    ? Number(buf.readBigUInt64LE(0))
    : buf.readUInt32LE(0);
  try {
    const helper = app.isPackaged
      ? path.join(process.resourcesPath, 'hud-set-override-redirect')
      : path.join(__dirname, 'hud-set-override-redirect');
    execSync(`"${helper}" ${winIdNum}`);
  } catch (e) {
    console.warn(`[HUD] override_redirect helper failed (non-fatal):`, (e as Error).message);
  }

  mainWindow.loadFile(path.join(__dirname, 'renderer/index.html'));
  positionWindow('bottom');

  mainWindow.once('ready-to-show', () => {
    mainWindow?.show();
  });

  ipcMain.handle('get-config', () => config);

  ipcMain.on('move-window', (_event, position: string) => {
    if (position === 'top' || position === 'center' || position === 'bottom') {
      positionWindow(position);
    }
  });

  const socket = dgram.createSocket('udp4');
  let socketClosed = false;

  function closeSocket(): void {
    if (!socketClosed) {
      socketClosed = true;
      socket.close();
    }
  }

  socket.on('message', (msg) => {
    const text = msg.toString().trim();
    if (!text) return;
    console.log('[UDP] Received:', text.substring(0, 200) + (text.length > 200 ? '...' : ''));
    // Parse JSON commands; fall back to plain-text display
    try {
      const cmd = JSON.parse(text);
      if (cmd.type === 'shutdown') {
        console.log('[UDP] Received shutdown command, quitting...');
        closeSocket();
        if (mainWindow && !mainWindow.isDestroyed()) mainWindow.close();
        app.quit();
        return;
      }
      if (cmd.type === 'position' && (cmd.pos === 'top' || cmd.pos === 'bottom')) {
        positionWindow(cmd.pos);
        return;
      }
      if (cmd.type === 'message' && typeof cmd.text === 'string') {
        // Extract inner text from the JSON envelope and display it
        if (mainWindow && !mainWindow.isDestroyed()) {
          mainWindow.webContents.send('hud-text-changed', cmd.text);
        }
        return;
      }
    } catch {
      // Not JSON — treat as plain text (backward compat)
    }
    if (mainWindow && !mainWindow.isDestroyed()) {
      mainWindow.webContents.send('hud-text-changed', text);
    }
  });

  socket.on('error', (err) => {
    console.error('UDP error:', err);
    closeSocket();
  });

  socket.bind(config.udp_port, '127.0.0.1', () => {
    const addr = socket.address();
    console.log(`HUD listening on UDP ${addr.address}:${addr.port}`);
  });

  app.on('before-quit', () => {
    closeSocket();
  });

  mainWindow.on('closed', () => {
    mainWindow = null;
  });
});

app.on('window-all-closed', () => {
  app.quit();
});
