export type WindowPosition = 'top' | 'center' | 'bottom';

export interface DisplayBounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

export function computePosition(
  display: DisplayBounds,
  windowSize: [number, number],
  position: WindowPosition,
  bottomMargin: number,
): { x: number; y: number } {
  const [winW, winH] = windowSize;
  const x = display.x + Math.floor((display.width - winW) / 2);
  let y: number;
  switch (position) {
    case 'top':
      y = display.y + bottomMargin;
      break;
    case 'center':
      y = display.y + Math.floor((display.height - winH) / 2);
      break;
    default:
      y = display.y + display.height - winH - bottomMargin;
  }
  return { x, y };
}
