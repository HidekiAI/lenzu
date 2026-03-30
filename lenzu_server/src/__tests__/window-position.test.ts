import { describe, it, expect } from 'vitest';
import { computePosition } from '../window-position';

const display = { x: 0, y: 0, width: 1920, height: 1080 };
const winSize: [number, number] = [1920, 200];
const margin = 50;

describe('computePosition — bottom', () => {
  it('bottom edge sits margin px above display bottom', () => {
    const { y } = computePosition(display, winSize, 'bottom', margin);
    expect(y + winSize[1] + margin).toBe(display.y + display.height);
  });

  it('window is horizontally centered', () => {
    const { x } = computePosition(display, winSize, 'bottom', margin);
    expect(x).toBe(display.x + Math.floor((display.width - winSize[0]) / 2));
  });
});

describe('computePosition — top', () => {
  it('top edge sits margin px below display top', () => {
    const { y } = computePosition(display, winSize, 'top', margin);
    expect(y).toBe(display.y + margin);
  });

  it('window is horizontally centered', () => {
    const { x } = computePosition(display, winSize, 'top', margin);
    expect(x).toBe(display.x + Math.floor((display.width - winSize[0]) / 2));
  });
});

describe('computePosition — center', () => {
  it('window is vertically centered', () => {
    const { y } = computePosition(display, winSize, 'center', margin);
    expect(y).toBe(display.y + Math.floor((display.height - winSize[1]) / 2));
  });

  it('window is horizontally centered', () => {
    const { x } = computePosition(display, winSize, 'center', margin);
    expect(x).toBe(display.x + Math.floor((display.width - winSize[0]) / 2));
  });
});

describe('computePosition — offset display (second monitor)', () => {
  const offset = { x: 1920, y: 100, width: 2560, height: 1440 };
  const win: [number, number] = [2560, 200];

  it('horizontal position is offset from display origin', () => {
    const { x } = computePosition(offset, win, 'bottom', 50);
    expect(x).toBe(offset.x + Math.floor((offset.width - win[0]) / 2));
  });

  it('vertical position is offset from display origin', () => {
    const { y } = computePosition(offset, win, 'bottom', 50);
    expect(y + win[1] + 50).toBe(offset.y + offset.height);
  });
});

describe('computePosition — narrow window centered on wide display', () => {
  it('centers a 800 px window on a 1920 px display', () => {
    const { x } = computePosition(display, [800, 200], 'bottom', margin);
    expect(x).toBe(560); // (1920 - 800) / 2
  });
});
