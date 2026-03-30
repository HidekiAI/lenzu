import { contextBridge, ipcRenderer } from 'electron';

interface HudConfig {
  background_opacity: number;
  text_color: string;
  font_size_pt: number;
  min_font_size_pt: number;
  default_text: string;
}

contextBridge.exposeInMainWorld('electronHUD', {
  getConfig: (): Promise<HudConfig> => ipcRenderer.invoke('get-config'),
  onTextChanged: (callback: (text: string) => void) => {
    ipcRenderer.on('hud-text-changed', (_event, text: string) => callback(text));
  },
  moveWindow: (position: string) => {
    ipcRenderer.send('move-window', position);
  },
});
