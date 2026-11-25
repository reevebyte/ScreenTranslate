import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, waitFor } from '@testing-library/svelte';
import { tick } from 'svelte';
import { previewBootstrap } from '$lib/preview';

const { call, toast } = vi.hoisted(() => ({ call: vi.fn(), toast: { error: vi.fn(), success: vi.fn(), warning: vi.fn(), dismiss: vi.fn() } }));
vi.mock('$lib/api', () => ({ desktop: true, call, loadBootstrap: vi.fn() }));
vi.mock('svelte-sonner', () => ({ toast }));
import { app } from '$lib/store.svelte';
import { setLocale } from '$lib/i18n.svelte';
import ShortcutsView from './ShortcutsView.svelte';

beforeEach(() => {
  vi.clearAllMocks();
  setLocale('zh-CN');
  vi.stubGlobal('matchMedia', () => ({ matches: false }));
  app.data = previewBootstrap(); app.configError = ''; app.configErrorRaw = ''; app.configErrorAction = ''; app.configPending = false;
  app.data.config.hotkeys.capture = 'Ctrl+Alt+Q';
});
afterEach(async () => { cleanup(); await tick(); });

it('names a taken hotkey in the shared notice, outlines its row and keeps the working key', async () => {
  const saved: string[] = [];
  call.mockImplementation(async (command, args) => {
    if (command !== 'save_config') return;
    saved.push(args.config.hotkeys.capture);
    if (args.config.hotkeys.capture === 'Ctrl+Alt+K') throw { code: 'configuration', message: '快捷键已被其他程序占用，请重新设置', action: 'hotkeys' };
    return args.config;
  });
  const view = render(ShortcutsView);
  const field = view.getByLabelText('框选翻译快捷键');
  await fireEvent.keyDown(field, { key: 'k', code: 'KeyK', ctrlKey: true, altKey: true });

  await waitFor(() => expect(toast.warning).toHaveBeenCalledWith('Ctrl + Alt + K 已被其他程序占用，已恢复原快捷键', { id: 'hotkey-capture' }));
  expect(field.closest('.shortcut-action')?.classList.contains('conflict')).toBe(true);
  // The reason lives in the notice only; no generic "settings not saved" notice is added.
  expect(toast.error).not.toHaveBeenCalled();
  expect(view.queryByText(/已被其他程序占用/)).toBeNull();
  // Restored and saved again, so the rejected key is not carried into later saves.
  await waitFor(() => expect(saved).toEqual(['Ctrl+Alt+K', 'Ctrl+Alt+Q']));
  await waitFor(() => expect(app.data!.config.hotkeys.capture).toBe('Ctrl+Alt+Q'));

  await fireEvent.keyDown(field, { key: 'j', code: 'KeyJ', ctrlKey: true, altKey: true });
  expect(field.closest('.shortcut-action')?.classList.contains('conflict')).toBe(false);
});

it('shows only the three core shortcut actions without quick search', () => {
  const view = render(ShortcutsView);
  expect(view.getByRole('button', { name: '框选翻译' })).toBeTruthy();
  expect(view.getByRole('button', { name: '收起 / 显示' })).toBeTruthy();
  expect(view.getByRole('button', { name: '快速文字翻译' })).toBeTruthy();
  expect(view.queryByRole('button', { name: '快速查找' })).toBeNull();
});

it('says when a hotkey duplicates another action instead of blaming another app', async () => {
  call.mockImplementation(async (command, args) => {
    if (command !== 'save_config') return;
    if (args.config.hotkeys.quick === 'Ctrl+Alt+Q') throw { code: 'configuration', message: '快捷键 Ctrl+Alt+Q 已用于其他操作', action: 'hotkeys' };
    return args.config;
  });
  app.data!.config.hotkeys.quick = 'Ctrl+Alt+Space';
  const view = render(ShortcutsView);
  await fireEvent.keyDown(view.getByLabelText('快速文字翻译快捷键'), { key: 'q', code: 'KeyQ', ctrlKey: true, altKey: true });
  await waitFor(() => expect(toast.warning).toHaveBeenCalledWith('Ctrl + Alt + Q 已用于其他操作，已恢复原快捷键', { id: 'hotkey-quick' }));
  await waitFor(() => expect(app.data!.config.hotkeys.quick).toBe('Ctrl+Alt+Space'));
});

it('localizes taken hotkey conflict notices in English UI mode', async () => {
  await setLocale('en');
  call.mockImplementation(async (command, args) => {
    if (command !== 'save_config') return;
    if (args.config.hotkeys.capture === 'Ctrl+Alt+K') throw { code: 'configuration', message: '快捷键已被其他程序占用，请重新设置', action: 'hotkeys' };
    return args.config;
  });
  const view = render(ShortcutsView);
  await fireEvent.keyDown(view.getByLabelText('Capture translation shortcut'), { key: 'k', code: 'KeyK', ctrlKey: true, altKey: true });
  await waitFor(() => expect(toast.warning).toHaveBeenCalledWith('Ctrl + Alt + K is used by another app; the previous shortcut was restored.', { id: 'hotkey-capture' }));
});
