import { beforeEach, expect, it, vi } from 'vitest';
import { previewBootstrap } from './preview';

const { call, loadBootstrap, toast } = vi.hoisted(() => ({
  call: vi.fn(),
  loadBootstrap: vi.fn(),
  toast: { error: vi.fn(), success: vi.fn(), warning: vi.fn() },
}));
vi.mock('./api', () => ({ desktop: true, call, loadBootstrap }));
vi.mock('svelte-sonner', () => ({ toast }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
import { app, initialize, retryConfigSave, saveConfig } from './store.svelte';

beforeEach(() => { vi.clearAllMocks(); app.data = previewBootstrap(); app.configError = ''; app.configErrorAction = ''; });

it('remembers why a save failed and retries exactly the failed configuration', async () => {
  const config = JSON.parse(JSON.stringify(app.data!.config));
  config.autoTranslate = !config.autoTranslate;
  call.mockImplementation(async command => {
    if (command === 'save_config') throw { code: 'configuration', message: '快捷键已被其他程序占用，请重新设置', action: 'hotkeys' };
  });
  expect(await saveConfig(config, { quiet: true })).toBe(false);
  expect(app.configError).toBe('快捷键已被其他程序占用，请重新设置');
  expect(app.configErrorAction).toBe('hotkeys');

  call.mockImplementation(async (command, args) => command === 'save_config' ? args.config : undefined);
  expect(await retryConfigSave()).toBe(true);
  const saves = call.mock.calls.filter(([command]) => command === 'save_config');
  expect(saves).toHaveLength(2);
  expect(saves[1][1]).toEqual({ config: expect.objectContaining({ autoTranslate: config.autoTranslate }) });
  expect(app.configError).toBe('');
  expect(app.configErrorAction).toBe('');
});

it('shows warnings raised during startup once in the main window', async () => {
  loadBootstrap.mockResolvedValue({ ...previewBootstrap(), warnings: ['原配置已备份为 config.broken-1.json，本次使用默认设置'] });
  (await initialize())();
  (await initialize())();
  expect(toast.warning).toHaveBeenCalledTimes(1);
  expect(toast.warning).toHaveBeenCalledWith('原配置已备份为 config.broken-1.json，本次使用默认设置');
});
