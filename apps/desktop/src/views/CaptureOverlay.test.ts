import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest';
import { render, fireEvent, waitFor, cleanup } from '@testing-library/svelte';
const { call, handlers } = vi.hoisted(() => ({
  call: vi.fn(),
  handlers: new Map<string, (event: { payload: unknown }) => void>(),
}));
vi.mock('$lib/api', () => ({ call }));
vi.mock('@tauri-apps/api/core', async original => ({
  ...(await original<typeof import('@tauri-apps/api/core')>()),
  isTauri: () => true,
}));
vi.mock('@tauri-apps/api/event', () => ({
  emit: vi.fn(async () => {}),
  listen: vi.fn(async (event: string, handler: (event: { payload: unknown }) => void) => {
    handlers.set(event, handler);
    return () => handlers.delete(event);
  }),
}));
import CaptureOverlay from './CaptureOverlay.svelte';

beforeEach(() => {
  call.mockReset();
  handlers.clear();
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => setTimeout(() => callback(0), 0));
  call.mockImplementation(async command => command === 'selection_frame'
    ? { id: 'screen', width: 2000, height: 1200, scaleFactor: 2, screenBounds: null, cleanId: null }
    : undefined);
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(1000);
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(600);
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} });
  Object.defineProperties(HTMLElement.prototype, {
    setPointerCapture: { value: vi.fn(), configurable: true },
    releasePointerCapture: { value: vi.fn(), configurable: true },
    hasPointerCapture: { value: () => true, configurable: true },
  });
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

describe('screenshot selection', () => {
  it('cancels the selection on Escape', async () => {
    render(CaptureOverlay);
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(call).toHaveBeenCalledWith('cancel_capture');
  });
  it('maps a reverse drag to image pixels and submits the selection once', async () => {
    const view = render(CaptureOverlay);
    await waitFor(() => expect(view.queryByAltText('待框选的屏幕图像')).toBeTruthy());
    const surface = view.container.querySelector('.capture-surface')!;
    await fireEvent.pointerDown(surface, { button: 0, pointerId: 1, clientX: 800, clientY: 500 });
    await fireEvent.pointerMove(surface, { pointerId: 1, clientX: 200, clientY: 100 });
    await fireEvent.pointerUp(surface, { button: 0, pointerId: 1, clientX: 200, clientY: 100 });
    await fireEvent.pointerUp(surface, { button: 0, pointerId: 1, clientX: 200, clientY: 100 });
    expect(call.mock.calls.filter(([name]) => name === 'select_region')).toEqual([
      ['select_region', { rect: { x: 400, y: 200, width: 1200, height: 800 } }],
    ]);
  });

  it('abandons an interrupted pointer gesture without submitting a partial region', async () => {
    const view = render(CaptureOverlay);
    await waitFor(() => expect(view.queryByAltText('待框选的屏幕图像')).toBeTruthy());
    const surface = view.container.querySelector('.capture-surface')!;
    await fireEvent.pointerDown(surface, { button: 0, pointerId: 2, clientX: 20, clientY: 20 });
    await fireEvent.pointerMove(surface, { pointerId: 2, clientX: 200, clientY: 150 });
    await fireEvent.pointerCancel(surface, { pointerId: 2 });
    await fireEvent.pointerUp(surface, { pointerId: 2, clientX: 200, clientY: 150 });
    expect(call.mock.calls.some(([name]) => name === 'select_region')).toBe(false);
    expect(view.container.querySelector('.selection-rect')).toBeNull();
  });

  it('paints an empty frame before the reused window is hidden', async () => {
    const view = render(CaptureOverlay);
    await waitFor(() => expect(view.queryByAltText('待框选的屏幕图像')).toBeTruthy());
    await waitFor(() => expect(handlers.has('selection-retire')).toBe(true));
    handlers.get('selection-retire')!({ payload: 7 });
    await waitFor(() => expect(call).toHaveBeenCalledWith('selection_cleared', { token: 7 }));
    expect(view.queryByAltText('待框选的屏幕图像')).toBeNull();
    expect(view.container.querySelector('.capture-guide')).toBeNull();
    expect(view.container.querySelector('.capture-shade')).toBeNull();
  });

  it('keeps a prewarmed window without a selection blank', async () => {
    call.mockImplementation(async command => {
      if (command === 'selection_frame') throw new Error('选区已过期');
    });
    const view = render(CaptureOverlay);
    await waitFor(() => expect(call).toHaveBeenCalledWith('window_ready', { frameId: null }));
    expect(view.container.querySelector('.capture-surface')!.children).toHaveLength(0);
  });

  it('waits for viewport resize before signaling window_ready when overlay starts undersized', async () => {
    let currentWidth = 800;
    let currentHeight = 600;
    vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockImplementation(() => currentWidth);
    vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockImplementation(() => currentHeight);

    const view = render(CaptureOverlay);
    const img = await view.findByAltText('待框选的屏幕图像');
    await fireEvent.load(img);
    await new Promise(resolve => setTimeout(resolve, 20));
    expect(call).not.toHaveBeenCalledWith('window_ready', { frameId: 'screen' });

    currentWidth = 1000;
    currentHeight = 600;
    window.dispatchEvent(new Event('resize'));
    await waitFor(() => expect(call).toHaveBeenCalledWith('window_ready', { frameId: 'screen' }));
    await waitFor(() => expect(call).toHaveBeenCalledWith('selection_presented'));
  });

  it('falls back to signaling window_ready after timeout if viewport resize never arrives', async () => {
    vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(800);
    vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(600);

    const view = render(CaptureOverlay);
    const img = await view.findByAltText('待框选的屏幕图像');
    await fireEvent.load(img);
    await waitFor(() => expect(call).toHaveBeenCalledWith('window_ready', { frameId: 'screen' }), { timeout: 500 });
  });

  it('lets the error banner return button cancel capture without pointer capture swallowing the click', async () => {
    const view = render(CaptureOverlay);
    const img = await view.findByAltText('待框选的屏幕图像');
    await fireEvent.error(img);
    const back = await view.findByRole('button', { name: '返回' });
    await fireEvent.pointerDown(back, { button: 0, pointerId: 1 });
    await fireEvent.click(back);
    expect(call).toHaveBeenCalledWith('cancel_capture');
  });
});
