import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, waitFor } from '@testing-library/svelte';
import { previewBootstrap } from '$lib/preview';

const { call, listen, focusInput } = vi.hoisted(() => ({ call: vi.fn(), listen: vi.fn(), focusInput: { current: null as (() => void) | null } }));
vi.mock('$lib/api', () => ({ desktop: true, call, loadBootstrap: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen }));
vi.mock('svelte-sonner', () => ({ toast: { error: vi.fn(), success: vi.fn() } }));
import { app } from '$lib/store.svelte';
import QuickView from './QuickView.svelte';

beforeEach(() => {
  vi.clearAllMocks();
  listen.mockImplementation(async (_name: string, callback: () => void) => { focusInput.current = callback; return vi.fn(); });
  app.data = previewBootstrap();
  app.input = 'Selected source';
  app.data.text.output = '译文';
});
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

it('unregisters a native listener that resolves after the tool was removed', async () => {
  let complete!: (stop: () => void) => void;
  listen.mockReturnValueOnce(new Promise<() => void>(resolve => complete = resolve));
  const stop = vi.fn();
  const view = render(QuickView);
  view.unmount();
  complete(stop);
  await waitFor(() => expect(stop).toHaveBeenCalledTimes(1));
});

it('leaves Ctrl+C in the input to the browser instead of copying the translation', async () => {
  const view = render(QuickView);
  const input = view.getByLabelText('快速翻译输入') as HTMLTextAreaElement;
  input.setSelectionRange(0, 8);
  const event = new KeyboardEvent('keydown', { key: 'c', ctrlKey: true, bubbles: true, cancelable: true });
  await fireEvent(input, event);
  expect(event.defaultPrevented).toBe(false);
  expect(call).not.toHaveBeenCalledWith('copy_text', expect.anything());
});

it('copies the translation with Ctrl+C outside the text editor', async () => {
  const view = render(QuickView);
  await fireEvent.keyDown(view.getByRole('button', { name: '复制译文' }), { key: 'c', ctrlKey: true });
  await waitFor(() => expect(call).toHaveBeenCalledWith('copy_text', { text: '译文' }));
});

it('reads both full texts without routing reading-dialog keys into translation shortcuts', async () => {
  const view = render(QuickView);
  await fireEvent.click(view.getByRole('button', { name: '阅读' }));
  const dialog = view.getByRole('dialog', { name: '原译对照' });
  expect(dialog.textContent).toContain('Selected source');
  expect(dialog.textContent).toContain('译文');
  await fireEvent.keyDown(dialog, { key: 'Escape' });
  expect(call).not.toHaveBeenCalledWith('hide_window');
});

it('uses readable tabs in a small large-font window and keeps tab Enter out of translation', async () => {
  vi.stubGlobal('innerHeight', 260);
  app.data!.config.appearance.fontScale = 1.5;
  const view = render(QuickView);
  const target = await view.findByRole('tab', { name: /^译文$/ });
  await fireEvent.keyDown(target, { key: 'Enter' });
  expect(call).not.toHaveBeenCalledWith('translate_text', expect.anything());
  await fireEvent.click(target);
  expect(view.getByRole('tabpanel', { name: '译文' }).textContent).toContain('译文');
});


it('returns to the source and focuses it when the native focus-input event reopens a compact tool', async () => {
  vi.stubGlobal('innerHeight', 260);
  app.data!.config.appearance.fontScale = 1.5;
  const view = render(QuickView);
  await fireEvent.click(await view.findByRole('tab', { name: /^译文$/ }));
  focusInput.current?.();
  await waitFor(() => {
    const input = view.getByLabelText('快速翻译输入') as HTMLTextAreaElement;
    expect(document.activeElement).toBe(input);
    expect(input.selectionEnd).toBe(input.value.length);
  });
});
