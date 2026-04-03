import { app, BrowserWindow, ipcMain, screen } from "electron";
import * as path from "path";
import * as dgram from "dgram";
import { loadConfig, DEFAULT_CONFIG, type HudConfig } from "./config";
import { computePosition, type WindowPosition } from "./window-position";

console.log("lenzu_server starting");
// Required for transparent windows on X11
app.commandLine.appendSwitch("enable-transparent-visuals");

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
    config = loadConfig(path.join(app.getAppPath(), "hud_config.json"));

    const primaryDisplay = screen.getPrimaryDisplay();
    const screenWidth = primaryDisplay.workAreaSize.width;

    mainWindow = new BrowserWindow({
        width: screenWidth,
        height: config.height,
        transparent: true,
        frame: false,
        alwaysOnTop: true,
        skipTaskbar: true,
        resizable: false,
        hasShadow: false,
        backgroundColor: "#00000000",
        webPreferences: {
            preload: path.join(__dirname, "preload.js"),
            contextIsolation: true,
            nodeIntegration: false,
        },
    });

    mainWindow.loadFile(path.join(__dirname, "renderer/index.html"));
    positionWindow("bottom");

    ipcMain.handle("get-config", () => config);

    ipcMain.on("move-window", (_event, position: string) => {
        if (position === "top" || position === "center" || position === "bottom") {
            positionWindow(position);
        }
    });

    const socket = dgram.createSocket("udp4");

    socket.on("message", (msg) => {
        const text = msg.toString().trim();
        // Log all incoming UDP messages for debugging
        console.log("[UDP] Received message:", text.substring(0, 200) + (text.length > 200 ? "..." : ""));
        if (text && mainWindow) {
            // Check for shutdown command
            if (text === '{"type":"shutdown"}') {
                console.log("[UDP] Received shutdown command, closing server...");
                socket.close();
                if (mainWindow) {
                    mainWindow.close();
                }
                app.quit();
                return;
            }
            mainWindow.webContents.send("hud-text-changed", text);
        }
    });

    socket.on("error", (err) => {
        console.error("UDP error:", err);
        socket.close();
    });

    socket.bind(config.udp_port, "127.0.0.1", () => {
        const addr = socket.address();
        console.log(`HUD listening on UDP ${addr.address}:${addr.port}`);
    });

    app.on("before-quit", () => {
        socket.close();
    });

    mainWindow.on("closed", () => {
        mainWindow = null;
    });
});

app.on("window-all-closed", () => {
    app.quit();
});
