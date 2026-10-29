import defaults from './default-config.json';
import type { Bootstrap, Config, TranslationSession } from './generated';

export function previewBootstrap(): Bootstrap {
  const config = structuredClone(defaults) as Config;
  config.onboardingComplete = true;
  const text: TranslationSession = {
    id: 'text', revision: 0, clientId: '', clientRevision: 0, input: '', output: '', source: 'auto', target: 'auto',
    frame: null, blocks: [], job: { id: '', sessionId: 'text', revision: 0, stage: 'idle', error: null },
  };
  return {
    version: '2.0.3', initialRoute: 'translate', config, text, visual: { ...text, id: 'visual' }, credentials: {}, warnings: [], monitors: [],
    legacyAvailable: false, updaterReady: false,
    capabilities: { platform: 'preview', wayland: false, screenshot: false, overlay: false, globalShortcuts: false,
      localOcr: false, ocrLanguages: [], tray: false, notes: ['浏览器仅预览界面；截图、OCR 与网络翻译请使用桌面应用。'] },
  };
}
