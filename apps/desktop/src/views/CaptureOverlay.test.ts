import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest';
import { render, fireEvent, waitFor, cleanup } from '@testing-library/svelte';
const { call } = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock('$lib/api', () => ({ call }));
import CaptureOverlay from './CaptureOverlay.svelte';

beforeEach(() => {
  call.mockReset();
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
});
