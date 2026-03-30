import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import { loadConfig, DEFAULT_CONFIG } from '../config';

describe('loadConfig', () => {
  let tmpDir: string;

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'hud-test-'));
  });

  afterEach(() => {
    fs.rmSync(tmpDir, { recursive: true });
  });

  it('returns defaults when file does not exist', () => {
    const cfg = loadConfig(path.join(tmpDir, 'missing.json'));
    expect(cfg).toEqual(DEFAULT_CONFIG);
  });

  it('merges file values over defaults', () => {
    const configPath = path.join(tmpDir, 'hud_config.json');
    fs.writeFileSync(configPath, JSON.stringify({ udp_port: 9999, text_color: '#ff0000' }));
    const cfg = loadConfig(configPath);
    expect(cfg.udp_port).toBe(9999);
    expect(cfg.text_color).toBe('#ff0000');
    expect(cfg.height).toBe(DEFAULT_CONFIG.height);
    expect(cfg.font_size_pt).toBe(DEFAULT_CONFIG.font_size_pt);
  });

  it('returns defaults when JSON is malformed', () => {
    const configPath = path.join(tmpDir, 'hud_config.json');
    fs.writeFileSync(configPath, 'not valid json {{{');
    const cfg = loadConfig(configPath);
    expect(cfg).toEqual(DEFAULT_CONFIG);
  });

  it('returns defaults when file is empty', () => {
    const configPath = path.join(tmpDir, 'hud_config.json');
    fs.writeFileSync(configPath, '');
    const cfg = loadConfig(configPath);
    expect(cfg).toEqual(DEFAULT_CONFIG);
  });

  it('does not mutate DEFAULT_CONFIG', () => {
    const before = { ...DEFAULT_CONFIG };
    const configPath = path.join(tmpDir, 'hud_config.json');
    fs.writeFileSync(configPath, JSON.stringify({ udp_port: 1234 }));
    loadConfig(configPath);
    expect(DEFAULT_CONFIG).toEqual(before);
  });
});
