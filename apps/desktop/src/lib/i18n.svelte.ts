import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export type UiLocale = 'zh-CN' | 'zh-TW' | 'en' | 'ja' | 'ko' | 'fr' | 'de' | 'es' | 'ru';

export const uiLocaleOptions: ReadonlyArray<readonly [UiLocale, string]> = [
  ['zh-CN', '简体中文'],
  ['zh-TW', '繁體中文'],
  ['en', 'English'],
  ['ja', '日本語'],
  ['ko', '한국어'],
  ['fr', 'Français'],
  ['de', 'Deutsch'],
  ['es', 'Español'],
  ['ru', 'Русский'],
];

const SUPPORTED: ReadonlySet<string> = new Set(uiLocaleOptions.map(([code]) => code));
const STORAGE_KEY = 'screentranslate-ui-language';

type LocaleDict = Record<string, string>;
const loaders: Partial<Record<UiLocale, () => Promise<{ default: LocaleDict }>>> = {
  'zh-TW': () => import('./locales/zh-TW.json'),
  en: () => import('./locales/en.json'),
  ja: () => import('./locales/ja.json'),
  ko: () => import('./locales/ko.json'),
  fr: () => import('./locales/fr.json'),
  de: () => import('./locales/de.json'),
  es: () => import('./locales/es.json'),
  ru: () => import('./locales/ru.json'),
};

const catalogs = $state<Partial<Record<UiLocale, LocaleDict>>>({});
const inflight = new Map<UiLocale, Promise<void>>();

export function normalizeLocale(value: string | null | undefined): UiLocale {
  if (!value) return 'zh-CN';
  if (SUPPORTED.has(value)) return value as UiLocale;
  const lower = value.trim().toLowerCase();
  if (lower.startsWith('zh-tw') || lower.startsWith('zh-hk') || lower.startsWith('zh-mo') || lower.startsWith('zh-hant')) return 'zh-TW';
  if (lower.startsWith('zh')) return 'zh-CN';
  if (lower.startsWith('ja')) return 'ja';
  if (lower.startsWith('ko')) return 'ko';
  if (lower.startsWith('fr')) return 'fr';
  if (lower.startsWith('de')) return 'de';
  if (lower.startsWith('es')) return 'es';
  if (lower.startsWith('ru')) return 'ru';
  if (lower.startsWith('en')) return 'en';
  return 'en';
}

function detectSystemLocale(): UiLocale {
  if (import.meta.env.MODE === 'test') return 'zh-CN';
  if (typeof navigator === 'undefined' || !navigator.language) return 'zh-CN';
  return normalizeLocale(navigator.language);
}

function stored(): UiLocale {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    return saved ? normalizeLocale(saved) : detectSystemLocale();
  } catch {
    return detectSystemLocale();
  }
}

export const uiLocale = $state<{ current: UiLocale }>({ current: stored() });

export function loadLocale(language: UiLocale): Promise<void> {
  if (language === 'zh-CN' || catalogs[language]) return Promise.resolve();
  const pending = inflight.get(language);
  if (pending) return pending;
  const loader = loaders[language];
  if (!loader) return Promise.resolve();
  const task = loader()
    .then(mod => { catalogs[language] = mod.default; })
    .catch(() => {})
    .finally(() => { inflight.delete(language); });
  inflight.set(language, task);
  return task;
}

export function translate(text: string, language: UiLocale, ...values: unknown[]): string {
  const message = language === 'zh-CN' ? text : (catalogs[language]?.[text] ?? text);
  return message.replace(/\{(\d+)\}/g, (match, index) =>
    Number(index) < values.length ? String(values[Number(index)]) : match,
  );
}

export function t(text: string, ...values: unknown[]): string {
  return translate(text, uiLocale.current, ...values);
}

function apply(language: UiLocale): Promise<void> {
  uiLocale.current = language;
  if (typeof document !== 'undefined') document.documentElement.lang = language;
  return loadLocale(language);
}

export function setLocale(value: string): Promise<void> {
  const language = normalizeLocale(value);
  const loaded = apply(language);
  try { localStorage.setItem(STORAGE_KEY, language); } catch { /* Storage may be unavailable. */ }
  if (isTauri()) void invoke('set_ui_locale', { locale: language }).catch(() => {});
  return loaded;
}

export function initializeLocale() {
  void apply(stored());
  let disposed = false;
  let stop: (() => void) | undefined;
  const onStorage = (event: StorageEvent) => {
    if (event.key === STORAGE_KEY) void apply(stored());
  };
  window.addEventListener('storage', onStorage);
  if (isTauri()) {
    void listen<{ language: UiLocale }>('ui-locale-changed', ({ payload }) => {
      if (!disposed && SUPPORTED.has(payload.language)) void apply(payload.language);
    }).then(fn => { if (disposed) fn(); else stop = fn; });
  }
  return () => {
    disposed = true;
    stop?.();
    window.removeEventListener('storage', onStorage);
  };
}
