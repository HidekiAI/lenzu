'use strict';

/**
 * preload.js — Context bridge
 *
 * Exposes a minimal, typed API to the renderer process via contextBridge.
 * No Node.js APIs are exposed directly to the renderer.
 */

const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('hudAPI', {
  /**
   * Subscribe to incoming caption text messages.
   * @param {(text: string) => void} callback
   */
  onMessage: (callback) => {
    ipcRenderer.on('hud:message', (_event, text) => callback(text));
  },

  /**
   * Subscribe to config updates pushed from the main process.
   * @param {(config: object) => void} callback
   */
  onConfig: (callback) => {
    ipcRenderer.on('hud:config', (_event, cfg) => callback(cfg));
  },

  /**
   * Subscribe to clear events (wipe all displayed lines).
   * @param {() => void} callback
   */
  onClear: (callback) => {
    ipcRenderer.on('hud:clear', () => callback());
  },

  /**
   * Request the current config from the main process.
   * @returns {Promise<object>}
   */
  getConfig: () => ipcRenderer.invoke('hud:getConfig'),
});
