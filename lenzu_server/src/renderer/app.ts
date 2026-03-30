export {};

interface HudConfig {
  background_opacity: number;
  text_color: string;
  font_size_pt: number;
  min_font_size_pt: number;
  default_text: string;
}

declare global {
  interface Window {
    electronHUD: {
      getConfig: () => Promise<HudConfig>;
      onTextChanged: (callback: (text: string) => void) => void;
      moveWindow: (position: string) => void;
    };
  }
}

const subtitleBox  = document.getElementById('subtitle-box') as HTMLDivElement;
const subtitleText = document.getElementById('subtitle-text') as HTMLParagraphElement;

let cfg: HudConfig = {
  background_opacity: 0.45,
  text_color: '#f5e642',
  font_size_pt: 24,
  min_font_size_pt: 0,
  default_text: '',
};

function renderText(text: string): void {
  subtitleText.textContent = text;
  subtitleText.style.color = cfg.text_color;
  subtitleText.style.fontSize = `${cfg.font_size_pt}pt`;
  subtitleBox.style.background = text
    ? `rgba(10, 10, 10, ${cfg.background_opacity})`
    : 'transparent';
}

const POSITIONS = ['top', 'center', 'bottom'] as const;
let posIndex = 2;

document.addEventListener('keydown', (e: KeyboardEvent) => {
  if (e.key === 'ArrowUp' && posIndex > 0) {
    posIndex--;
    window.electronHUD.moveWindow(POSITIONS[posIndex]);
  } else if (e.key === 'ArrowDown' && posIndex < POSITIONS.length - 1) {
    posIndex++;
    window.electronHUD.moveWindow(POSITIONS[posIndex]);
  }
});

async function init(): Promise<void> {
  cfg = await window.electronHUD.getConfig();
  renderText(cfg.default_text);
  window.electronHUD.onTextChanged((text: string) => {
    renderText(text);
  });
}

init().catch(console.error);
