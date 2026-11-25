import { afterEach, expect, it } from 'vitest';
import { render, fireEvent, cleanup, waitFor } from '@testing-library/svelte';
import { translate, setLocale, uiLocale, uiLocaleOptions, loadLocale, normalizeLocale, type UiLocale } from './i18n.svelte';
import { app } from './store.svelte';
import { previewBootstrap } from './preview';
import SettingsView from '../views/SettingsView.svelte';
import en from './locales/en.json';
import zhTw from './locales/zh-TW.json';
import ja from './locales/ja.json';
import ko from './locales/ko.json';
import fr from './locales/fr.json';
import de from './locales/de.json';
import es from './locales/es.json';
import ru from './locales/ru.json';

afterEach(async () => {
  cleanup();
  await setLocale('zh-CN');
});

it('changes existing settings immediately and persists only the interface language', async () => {
  await setLocale('zh-CN');
  app.data = previewBootstrap();
  app.settingsTab = 'general';
  app.input = '原文不会变';
  const config = JSON.stringify(app.data.config);
  const view = render(SettingsView);
  await fireEvent.click(view.getByRole('combobox', { name: '界面语言' }));
  await fireEvent.pointerUp(view.getByRole('option', { name: 'English' }), { button: 0, pointerType: 'mouse' });
  await waitFor(() => expect(view.getByRole('tab', { name: 'General' })).toBeTruthy());
  expect(view.getByText('Translate as you type')).toBeTruthy();
  expect(uiLocale.current).toBe('en');
  expect(localStorage.getItem('screentranslate-ui-language')).toBe('en');
  expect(app.input).toBe('原文不会变');
  expect(JSON.stringify(app.data.config)).toBe(config);
});

it('lazy-loads additional languages and preserves user content', async () => {
  await loadLocale('en');
  expect(translate('发现新版本 {0}', 'en', '2.0.1')).toBe('Version 2.0.1 is available');
  expect(translate('设置', 'en')).toBe('Settings');
  expect(translate('This is user content', 'en')).toBe('This is user content');

  await setLocale('ja');
  expect(uiLocale.current).toBe('ja');
  expect(translate('设置', 'ja')).toBe((ja as Record<string, string>)['设置']);
  expect(normalizeLocale('zh-HK')).toBe('zh-TW');
  expect(normalizeLocale('fr-CA')).toBe('fr');
});

it('maintains 100% key and placeholder parity across all 9 UI locales', () => {
  expect(uiLocaleOptions).toHaveLength(9);
  const baseKeys = Object.keys(en);
  const placeholders = (text: string) => [...text.matchAll(/\{\d+\}/g)].map(m => m[0]).sort();
  const packs: Record<Exclude<UiLocale, 'zh-CN'>, Record<string, string>> = {
    'zh-TW': zhTw,
    en,
    ja,
    ko,
    fr,
    de,
    es,
    ru,
  };
  for (const [locale, dict] of Object.entries(packs)) {
    const keys = Object.keys(dict);
    expect(keys, `${locale} keys must match en.json`).toEqual(baseKeys);
    for (const key of baseKeys) {
      const value = dict[key];
      expect(typeof value === 'string' && value.trim().length > 0, `${locale}[${key}] must be non-empty`).toBe(true);
      expect(placeholders(value), `${locale}[${key}] placeholders must match`).toEqual(placeholders(key));
    }
  }
});
