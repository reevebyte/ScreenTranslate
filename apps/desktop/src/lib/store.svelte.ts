import { t } from './i18n.svelte';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { toast } from 'svelte-sonner';
import { call, desktop, loadBootstrap } from './api';
import { errorMessage } from './utils';
import { actionPalette } from './appearance';
import type { AppError, Bootstrap, Config, PlatformCapabilities, TranslationSession, Theme } from './generated';

export type Route = 'shortcuts' | 'translate' | 'capture' | 'providers' | 'settings' | 'about' | 'components';
export const app = $state({
  data: null as Bootstrap | null,
  route: 'translate' as Route,
  servicesTab: 'translation',
  settingsTab: 'general',
  fatal: '',
  input: '', source: 'auto', target: 'auto',
  commandOpen: false,
  configSaving: false,
  configPending: false,
  configError: '',
});

let configIntent = 0;
let configWrites = 0;
let configQueue: Promise<unknown> = Promise.resolve();
let stagedConfig = false;

function publishConfigActivity() {
  if (desktop) void call('set_dirty', { dirty: app.configPending || app.configSaving }).catch(() => {});
}

export function previewConfig(config: Config) {
  configIntent++;
  stagedConfig = true;
  app.configPending = true;
  app.configError = '';
  if (app.data) app.data.config = structuredClone($state.snapshot(config));
  applyAppearance(config);
  publishConfigActivity();
}

const clientId = crypto.randomUUID();
let localRevision = 0;
let pendingRevision = 0;
let latestSync: Promise<unknown> = Promise.resolve();
const bufferedSessions = new Map<string, TranslationSession>();

export function applySession(session: TranslationSession) {
  if (!app.data) { bufferedSessions.set(session.id, session); return; }
  if (session.id === 'text') {
    if (session.clientId === clientId && session.clientRevision < localRevision) return;
    if (session.revision < app.data.text.revision) return;
    app.data.text = session;
    if (pendingRevision === 0 || (session.clientId === clientId && session.clientRevision >= pendingRevision)) {
      app.input = session.input; app.source = session.source; app.target = session.target;
    }
  } else if (session.id === 'visual' && session.revision >= app.data.visual.revision) {
    app.data.visual = session;
  }
}

export async function initialize(): Promise<() => void> {
  const cleanup: UnlistenFn[] = [];
  try {
    if (desktop) {
      cleanup.push(await listen<TranslationSession>('session-changed', ({ payload }) => applySession(payload)));
      cleanup.push(await listen<Config>('config-changed', ({ payload }) => { if (app.data && !configWrites && !stagedConfig) app.data.config = payload; }));
      cleanup.push(await listen<Theme>('theme-previewed', ({ payload }) => { if (app.data && (new URLSearchParams(location.search).get('window') || 'main') !== 'main') { app.data.config.appearance.theme = payload; applyAppearance(app.data.config); } }));
      cleanup.push(await listen<AppError>('app-error', ({ payload }) => {
        toast.error(payload.message, { action: payload.action ? { label: t('查看设置'), onClick: () => recover(payload.action!) } : undefined });
      }));
      cleanup.push(await listen<string>('app-warning', ({ payload }) => toast.warning(payload)));
      cleanup.push(await listen('capabilities-changed', () => { void refreshCapabilities(); }));
    }
    const data = await loadBootstrap();
    app.data = data;
    if (['shortcuts', 'translate', 'capture', 'providers', 'settings', 'about'].includes(data.initialRoute)) app.route = data.initialRoute as Route;
    if (app.route === 'about') { app.route = 'settings'; app.settingsTab = 'about'; }
    if (data.initialRoute === 'ocr') { app.route = 'providers'; app.servicesTab = 'ocr'; }
    app.input = data.text.input; app.source = data.text.source; app.target = data.text.target;
    bufferedSessions.forEach(session => applySession(session));
    bufferedSessions.clear();
  } catch (error) { app.fatal = errorMessage(error); }
  return () => cleanup.forEach(fn => fn());
}

export function editText(input: string, source = app.source, target = app.target, composing = false) {
  app.input = input; app.source = source; app.target = target;
  if (app.data) { app.data.text.output = ''; app.data.text.job = { ...app.data.text.job, stage: 'idle', error: null }; }
  if (!desktop) return;
  const revision = ++localRevision;
  pendingRevision = revision;
  latestSync = call<TranslationSession>('set_text', { input, source, target, composing, clientId, clientRevision: revision })
    .then(session => { if (revision === localRevision) pendingRevision = 0; applySession(session); })
    .catch(error => { if (revision === localRevision) pendingRevision = 0; toast.error(errorMessage(error)); });
}

export async function translateNow() {
  await latestSync;
  if (!app.input.trim()) return;
  return act(call('translate_text', { revision: app.data?.text.revision ?? 0 }));
}

export async function act<T>(operation: Promise<T>): Promise<T | undefined> {
  try { return await operation; } catch (error) { toast.error(errorMessage(error)); return undefined; }
}

export async function copy(text: string) {
  if (!text) return false;
  try { await call('copy_text', { text }); return true; }
  catch (error) { toast.error(errorMessage(error)); return false; }
}

export async function saveConfig(config: Config, options: { quiet?: boolean } = {}): Promise<boolean> {
  const snapshot = structuredClone($state.snapshot(config));
  const intent = ++configIntent;
  configWrites++;
  stagedConfig = false;
  app.configSaving = true;
  app.configPending = true;
  publishConfigActivity();
  const operation = configQueue.then(async () => {
    try {
      const saved = desktop ? await call<Config>('save_config', { config: snapshot }) : snapshot;
      if (intent === configIntent) {
        if (app.data) app.data.config = saved;
        app.configError = '';
        app.configPending = false;
        applyAppearance(saved);
      }
      return true;
    } catch (error) {
      if (intent === configIntent) app.configError = errorMessage(error);
      if (!options.quiet) toast.error(errorMessage(error));
      return false;
    } finally {
      configWrites--;
      app.configSaving = configWrites > 0;
      publishConfigActivity();
    }
  });
  configQueue = operation;
  return operation;
}

let nativeTheme: string | undefined;
let desiredNativeTheme: Theme | undefined;
let syncingTheme = false;
async function syncNativeTheme() {
  if (syncingTheme) return;
  syncingTheme = true;
  try {
    while (desiredNativeTheme && desiredNativeTheme !== nativeTheme) {
      const theme = desiredNativeTheme;
      await call('preview_theme', { theme });
      nativeTheme = theme;
    }
  } catch { nativeTheme = undefined; }
  finally { syncingTheme = false; }
}
let appearanceFrame = 0;
export function applyAppearance(config: Config) {
  const theme = config.appearance.theme;
  const dark = theme === 'dark' || (theme === 'system' && matchMedia('(prefers-color-scheme: dark)').matches);
  if (desktop && (new URLSearchParams(location.search).get('window') || 'main') === 'main') { desiredNativeTheme = theme; void syncNativeTheme(); }
  const root = document.documentElement;
  if (root.classList.contains('dark') !== dark) {
    if (appearanceFrame) cancelAnimationFrame(appearanceFrame);
    root.classList.add('theme-changing');
    appearanceFrame = requestAnimationFrame(() => {
      appearanceFrame = requestAnimationFrame(() => { root.classList.remove('theme-changing'); appearanceFrame = 0; });
    });
  }
  root.classList.toggle('dark', dark);
  const palette = actionPalette(config.appearance.accent, dark);
  document.documentElement.style.setProperty('--primary', palette.primary);
  document.documentElement.style.setProperty('--primary-foreground', palette.foreground);
  document.documentElement.style.setProperty('--focus', palette.focus);
  document.documentElement.style.setProperty('--font-scale', String(config.appearance.fontScale));
  try { localStorage.setItem('screentranslate-theme', theme); } catch { /* System storage may be disabled. */ }
}

export async function refreshCapabilities() {
  const capabilities = await act(call<PlatformCapabilities>('refresh_capabilities'));
  if (capabilities && app.data) app.data.capabilities = capabilities;
}

export function recover(action: string) {
  const route = action === 'ocr' ? 'ocr' : action === 'providers' ? 'providers' : 'settings';
  if (action === 'ocr') app.servicesTab = 'ocr';
  if (desktop && (new URLSearchParams(location.search).get('window') || 'main') !== 'main' && !['permissions', 'releases'].includes(action)) {
    void act(call('navigate_main', { route })); return;
  }
  if (action === 'providers' || action === 'ocr') app.route = 'providers';
  else if (action === 'releases') void act(call('open_help', { topic: 'releases' }));
  else if (action === 'permissions') void act(call('open_help', { topic: 'permissions' }));
  else app.route = 'settings';
}
