import * as fs from 'fs';

export interface HudConfig {
  height: number;
  background_opacity: number;
  text_color: string;
  font_size_pt: number;
  min_font_size_pt: number;
  bottom_margin: number;
  udp_port: number;
  default_text: string;
}

export const DEFAULT_CONFIG: HudConfig = {
  height: 200,
  background_opacity: 0.72,
  text_color: '#f5e642',
  font_size_pt: 24,
  min_font_size_pt: 0,
  bottom_margin: 50,
  udp_port: 7331,
  default_text: 'Hello world, Hello Shiroe!',
};

export function loadConfig(configPath: string): HudConfig {
  try {
    const raw = fs.readFileSync(configPath, 'utf-8');
    return { ...DEFAULT_CONFIG, ...JSON.parse(raw) };
  } catch {
    return { ...DEFAULT_CONFIG };
  }
}
