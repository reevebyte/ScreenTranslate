import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, waitFor } from '@testing-library/svelte';
import { tick } from 'svelte';
import { previewBootstrap } from '$lib/preview';

const { call } = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock('$lib/api', () => ({ desktop: true, call, loadBootstrap: vi.fn() }));
const { toast } = vi.hoisted(() => ({ toast: { error: vi.fn(), success: vi.fn(), warning: vi.fn(), dismiss: vi.fn() } }));
vi.mock('svelte-sonner', () => ({ toast }));
import { app, previewConfig, saveConfig } from '$lib/store.svelte';
import { updates } from '$lib/updater.svelte';
import SettingsView from './SettingsView.svelte';

beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal('matchMedia', () => ({ matches: false }));
  call.mockImplementation(async (command, args) => command === 'save_config' ? args.config : undefined);
  app.data = previewBootstrap(); app.configError = ''; app.configPending = false;
  app.settingsTab = 'general';
});
afterEach(async () => { cleanup(); await tick(); });
describe('automatic preferences', () => {
  it('keeps three categories and saves advanced preferences without exposing an unavailable import', async () => {
    app.settingsTab = 'data';
    const view = render(SettingsView);
    expect(view.getAllByRole('tab').map(node => node.textContent?.trim())).toEqual(['常规', '外观', '关于']);
    expect(view.queryByText('导入旧版设置')).toBeNull();
    await fireEvent.click(view.getByText('更多设置'));
    await fireEvent.click(view.getByLabelText('截图译文自动复制'));
    view.unmount();
    await waitFor(() => expect(call).toHaveBeenCalledWith('save_config', { config: expect.objectContaining({ appearance: expect.objectContaining({ autoCopy: true }) }) }));
  });
  it('asks before restoring defaults and keeps settings when cancelled', async () => {
    app.data!.config.autoTranslate = false;
    const view = render(SettingsView);
    await fireEvent.click(view.getByRole('button', { name: '恢复' }));
    const dialog = await view.findByRole('dialog');
    expect(dialog.textContent).toContain('已保存的密钥会保留');
    await fireEvent.click(view.getByRole('button', { name: '取消' }));
    await waitFor(() => expect(view.queryByRole('dialog')).toBeNull());
    expect((view.getByLabelText('输入后自动翻译') as HTMLElement).getAttribute('data-state')).toBe('unchecked');
    await fireEvent.click(view.getByRole('button', { name: '恢复' }));
    const confirm = (await view.findByRole('dialog')).querySelector('.dialog-actions button:last-child') as HTMLElement;
    await fireEvent.click(confirm);
    await waitFor(() => expect((view.getByLabelText('输入后自动翻译') as HTMLElement).getAttribute('data-state')).toBe('checked'));
  });
  it('keeps automatic update checks connected to the saved preference in the centered about page', async () => {
    const view = render(SettingsView);
    await fireEvent.click(view.getByRole('tab', { name: '关于' }));
    await fireEvent.click(view.getByLabelText('自动检查更新'));
    view.unmount();
    await waitFor(() => expect(call).toHaveBeenCalledWith('save_config', { config: expect.objectContaining({ autoUpdate: false }) }));
  });
  it('steps the interface font within its limits and saves when leaving the page', async () => {
    const view = render(SettingsView);
    await fireEvent.click(view.getByRole('tab', { name: '外观' }));
    const plus = view.getByRole('button', { name: '增大界面字号' });
    for (let step = 0; step < 12; step++) await fireEvent.click(plus);
    expect(view.getByRole('status', { name: '界面字号' }).textContent).toBe('150%');
    expect((plus as HTMLButtonElement).disabled).toBe(true);
    await fireEvent.click(view.getByRole('button', { name: '减小界面字号' }));
    view.unmount();
    await waitFor(() => expect(call).toHaveBeenCalledWith('save_config', { config: expect.objectContaining({ appearance: expect.objectContaining({ fontScale: 1.45 }) }) }));
  });
  it('does not write on mount and flushes an edit on page removal', async () => {
    const view = render(SettingsView);
    await tick();
    expect(call.mock.calls.filter(([command]) => command === 'save_config')).toHaveLength(0);
    await fireEvent.click(view.getByLabelText('输入后自动翻译'));
    view.unmount();
    await waitFor(() => expect(call).toHaveBeenCalledWith('save_config', { config: expect.objectContaining({ autoTranslate: false }) }));
  });
  it('shows save failures and retries the edited value without a success toast', async () => {
    let attempts = 0;
    call.mockImplementation(async (command, args) => {
      if (command !== 'save_config') return;
      if (++attempts === 1) throw { message: '磁盘暂时不可用' };
      return args.config;
    });
    const view = render(SettingsView);
    await fireEvent.click(view.getByLabelText('输入后自动翻译'));
    // The shared notice states the reason and offers retry; the page itself shows no banner.
    await waitFor(() => expect(toast.error).toHaveBeenCalledWith('设置未保存', expect.objectContaining({ id: 'config-save', description: '磁盘暂时不可用' })));
    expect(view.queryByRole('alert')).toBeNull();
    const notice = toast.error.mock.calls.find(([title]) => title === '设置未保存')![1];
    notice.action.onClick();
    await waitFor(() => expect(toast.dismiss).toHaveBeenCalledWith('config-save'));
    expect(toast.success).not.toHaveBeenCalled();
    expect(app.data!.config.autoTranslate).toBe(false);
    expect(attempts).toBe(2);
  });
});

it('keeps the newest theme when an older save completes late', async () => {
  let finish!: () => void;
  call.mockImplementation(async (command, args) => {
    if (command === 'save_config') return new Promise(resolve => { finish = () => resolve(args.config); });
  });
  const older = previewBootstrap().config;
  older.appearance.theme = 'dark';
  previewConfig(older);
  const saving = saveConfig(older, { quiet: true });
  await waitFor(() => expect(finish).toBeTypeOf('function'));
  const newest = structuredClone(older);
  newest.appearance.theme = 'light';
  previewConfig(newest);
  finish();
  await saving;
  expect(app.data!.config.appearance.theme).toBe('light');
  expect(document.documentElement.classList.contains('dark')).toBe(false);
});

it('offers the desktop system theme accent swatch when exposed by capabilities', async () => {
  app.data!.capabilities.systemAccent = '#89b4fa';
  app.settingsTab = 'appearance';
  const view = render(SettingsView);
  const swatch = view.getByRole('button', { name: '系统主题' });
  await fireEvent.click(swatch);
  view.unmount();
  await waitFor(() => expect(call).toHaveBeenCalledWith('save_config', { config: expect.objectContaining({ appearance: expect.objectContaining({ accent: '#89b4fa' }) }) }));
});

it('omits the ready status subtext below the restart-to-update button in About', async () => {
  Object.assign(updates, { stage: 'ready', version: '2.0.43', error: '' });
  try {
    app.settingsTab = 'about';
    const view = render(SettingsView);
    expect(view.getByRole('button', { name: '重启更新' })).toBeTruthy();
    expect(view.queryByText('更新已就绪')).toBeNull();
  } finally {
    Object.assign(updates, { stage: 'idle', version: '', error: '' });
  }
});

it('swaps and saves the bidirectional smart translation language pair', async () => {
  const view = render(SettingsView);
  expect(view.getByRole('combobox', { name: '第一语言' }).textContent).toContain('简体中文');
  expect(view.getByRole('combobox', { name: '第二语言' }).textContent).toContain('英语');
  await fireEvent.click(view.getByRole('button', { name: '互换语言' }));
  expect(view.getByRole('combobox', { name: '第一语言' }).textContent).toContain('英语');
  expect(view.getByRole('combobox', { name: '第二语言' }).textContent).toContain('简体中文');
  view.unmount();
  await waitFor(() => expect(call).toHaveBeenCalledWith('save_config', {
    config: expect.objectContaining({ primaryTarget: 'en', secondaryTarget: 'zh-Hans' }),
  }));
});

