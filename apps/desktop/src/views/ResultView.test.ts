import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, fireEvent, cleanup, waitFor } from '@testing-library/svelte';
import { tick } from 'svelte';
import { previewBootstrap } from '$lib/preview';

const { call, startDragging, startResizeDragging, renderTranslatedImagePng, events } = vi.hoisted(() => ({
  call: vi.fn().mockResolvedValue(undefined),
  startDragging: vi.fn().mockResolvedValue(undefined),
  startResizeDragging: vi.fn().mockResolvedValue(undefined),
  renderTranslatedImagePng: vi.fn().mockResolvedValue(new Uint8Array([137, 80, 78, 71])),
  events: new Map<string,Set<(event:{payload:unknown})=>void>>(),
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async (name:string, fn:(event:{payload:unknown})=>void) => {
  const listeners=events.get(name)??new Set();listeners.add(fn);events.set(name,listeners);return ()=>listeners.delete(fn);
}) }));
vi.mock('$lib/api', () => ({ desktop: true, call, loadBootstrap: vi.fn() }));
vi.mock('$lib/overlay-layout', async (importOriginal) => {
  const actual = await importOriginal<typeof import('$lib/overlay-layout')>();
  return { ...actual, renderTranslatedImagePng };
});
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ startDragging, startResizeDragging, innerSize: vi.fn(async () => ({width:window.innerWidth*devicePixelRatio,height:window.innerHeight*devicePixelRatio})) }) }));
vi.mock('svelte/motion', () => ({ prefersReducedMotion: { current: true } }));
vi.mock('svelte-sonner', () => ({ toast: { error: vi.fn(), success: vi.fn() } }));
import { app } from '$lib/store.svelte';
import ResultView from './ResultView.svelte';

function broadcast(name:string,payload:unknown) { events.get(name)?.forEach(fn=>fn({payload})); }
function renderResult() {
  const image=render(ResultView,{floating:true});
  render(ResultView,{floating:true,toolsOnly:true});
  return image;
}

beforeEach(() => {
  cleanup(); events.clear(); vi.clearAllMocks();
  call.mockImplementation(async (command,args) => {
    if(command==='result_layout') return {frameId:'test-frame',controlsAbove:false,imageAlignLeft:false};
    if(command==='result_display')return {frameId:'test-frame',original:false,comparing:false,holdingOriginal:false};
    if(command==='set_result_display')broadcast('result-display-changed',{frameId:'test-frame',...args});
    if(command==='show_result_notice')broadcast('result-feedback',args);
    if(command==='toolbar_placement')return {above:false,alignLeft:false};
  });
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => setTimeout(()=>callback(performance.now()),0));
  vi.stubGlobal('cancelAnimationFrame', (id:ReturnType<typeof setTimeout>)=>clearTimeout(id));
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} });
  app.data = previewBootstrap();
  app.data.visual.frame = { id: 'test-frame', width: 640, height: 400, screenBounds: null, scaleFactor: 1, cleanId: null };
  app.data.visual.job.stage = 'recognizing';
});

afterEach(() => { cleanup(); vi.useRealTimers(); });

describe('floating screenshot recovery', () => {
  it('renders in main window when not floating', async () => {
    const view = render(ResultView, { floating: false });
    expect(view.container).not.toBeNull();
  });
  it('leaves Space activation on toolbar buttons to the focused control', async () => {
    app.data!.visual.output = 'translation';
    app.data!.visual.job.stage = 'complete';
    const view = renderResult();
    await waitFor(() => expect(events.get('result-display-changed')?.size).toBe(2));
    const event = new KeyboardEvent('keydown', { key: ' ', bubbles: true, cancelable: true });
    await fireEvent(view.getByRole('button', { name: '复制译图' }), event);
    expect(event.defaultPrevented).toBe(false);
    expect(call.mock.calls.filter(([command]) => command === 'set_result_display')).toHaveLength(0);
  });
  it('does not send display updates when releasing a space used by an editor', async () => {
    render(ResultView, { floating: true });
    await fireEvent.keyUp(window, { key: ' ' });
    expect(call.mock.calls.filter(([command]) => command === 'set_result_display')).toHaveLength(0);
  });
  it('keeps all selection anchors and restores tools only after the native image drag finishes', async () => {
    let release!:()=>void;
    startDragging.mockImplementationOnce(()=>new Promise<void>(resolve=>release=resolve));
    const view=render(ResultView,{floating:true});
    await fireEvent.pointerDown(view.getByAltText('截图原文'),{button:0});
    await waitFor(()=>expect(startDragging).toHaveBeenCalledOnce());
    expect(call).toHaveBeenCalledWith('begin_result_drag');
    expect(call).not.toHaveBeenCalledWith('finish_result_drag');
    expect(view.container.querySelectorAll('.resize-handle')).toHaveLength(8);
    expect(view.container.querySelector('.result-selection-border')).not.toBeNull();
    release();
    await waitFor(()=>expect(call).toHaveBeenCalledWith('finish_result_drag'));
  });
  it('restores tools when starting a native image drag fails', async () => {
    startDragging.mockRejectedValueOnce(new Error('native drag failed'));
    const view=render(ResultView,{floating:true});
    await fireEvent.pointerDown(view.getByAltText('截图原文'),{button:0});
    await waitFor(()=>expect(call).toHaveBeenCalledWith('finish_result_drag'));
    expect(view.getByRole('alert').textContent).toContain('无法移动截图');
  });
  it('copies recognized source or translated text from the compact menu without opening a reading dialog', async () => {
    app.data!.visual.input = 'Recognized original';
    app.data!.visual.output = '译文';
    app.data!.visual.job.stage = 'complete';
    const view = renderResult();
    await waitFor(()=>expect(events.get('result-feedback')?.size).toBe(1));
    expect(view.queryByRole('button', { name: '展开阅读' })).toBeNull();
    expect(view.queryByRole('button', { name: '另存为' })).toBeNull();
    await fireEvent.click(view.getByRole('button', { name: '复制文字' }));
    expect(call).toHaveBeenCalledWith('set_toolbar_menu_open', { open: true });
    const items = view.getAllByRole('menuitem').map(node => node.textContent?.trim());
    expect(items).toEqual(['原文', '译文']);
    await fireEvent.click(view.getByRole('menuitem', { name: '原文' }));
    await waitFor(() => expect(call).toHaveBeenCalledWith('copy_text', { text: 'Recognized original' }));
    expect(call).toHaveBeenCalledWith('set_toolbar_menu_open', { open: false });
    expect(view.queryByRole('dialog')).toBeNull();
    expect(view.getByRole('button',{name:'复制文字'}).querySelector('.copy-done')).not.toBeNull();
    expect(view.getByRole('button',{name:'复制译图'}).querySelector('.copy-done')).toBeNull();
    expect(call).not.toHaveBeenCalledWith('retry_visual');
  });
  it('keeps image and text copy feedback timers independent',async()=>{
    app.data!.visual.input='Original';app.data!.visual.output='译文';app.data!.visual.job.stage='complete';
    const view=renderResult();
    await waitFor(()=>expect(events.get('result-feedback')?.size).toBe(1));
    vi.useFakeTimers();
    const imageBtn=view.getByRole('button',{name:'复制译图'}),textBtn=view.getByRole('button',{name:'复制文字'});
    await fireEvent.click(imageBtn);
    await waitFor(() => expect(call).toHaveBeenCalledWith('copy_image', { png: [137, 80, 78, 71] }));
    await vi.advanceTimersByTimeAsync(700);
    await fireEvent.click(textBtn);
    await fireEvent.click(view.getByRole('menuitem', { name: '译文' }));
    await waitFor(() => expect(call).toHaveBeenCalledWith('copy_text', { text: '译文' }));
    expect(imageBtn.querySelector('.copy-done')).not.toBeNull();
    expect(textBtn.querySelector('.copy-done')).not.toBeNull();
    await vi.advanceTimersByTimeAsync(701);
    expect(imageBtn.querySelector('.copy-done')).toBeNull();
    expect(textBtn.querySelector('.copy-done')).not.toBeNull();
    await vi.advanceTimersByTimeAsync(700);
    expect(textBtn.querySelector('.copy-done')).toBeNull();
  });
  it('can extract OCR text while the translation request is still pending', async () => {
    app.data!.visual.input = 'Already recognized';
    app.data!.visual.job.stage = 'translating';
    const view = renderResult();
    const button = view.getByRole('button', { name: '复制文字' }) as HTMLButtonElement;
    expect(button.disabled).toBe(false);
    await fireEvent.click(button);
    expect((view.getByRole('menuitem', { name: '译文' }) as HTMLButtonElement).disabled).toBe(true);
    await fireEvent.click(view.getByRole('menuitem', { name: '原文' }));
    await waitFor(() => expect(call).toHaveBeenCalledWith('copy_text', { text: 'Already recognized' }));
    expect(call).not.toHaveBeenCalledWith('cancel_job', expect.anything());
  });
  it('shows a brief compact error and keeps the full reason on the recovery control', async () => {
    vi.useFakeTimers();
    app.data!.visual.job = { ...app.data!.visual.job, stage: 'error', error: { code: 'configuration', message: '服务地址必须使用 HTTPS；本机服务可以使用 HTTP', action: 'providers' } };
    const view = renderResult();
    await tick();
    expect(view.getByRole('alert').textContent).toBe('请检查服务配置');
    expect(view.getByRole('button', { name: '打开服务设置' }).title).toContain('HTTPS');
    await vi.advanceTimersByTimeAsync(2800);
    await tick();
    expect(view.queryByRole('alert')).toBeNull();
    expect(view.getByRole('button', { name: '打开服务设置' })).toBeTruthy();
  });
  it('docks the toolbar with its own grip without dragging the image window', async () => {
    const view = renderResult();
    const grip = view.getByRole('button', { name: '移动工具栏' });
    await fireEvent.keyDown(grip, { key: 'ArrowUp' });
    await waitFor(() => expect(call).toHaveBeenCalledWith('set_result_toolbar', { above: true, alignLeft: false }));
    expect(startDragging).not.toHaveBeenCalled();
  });

  it('serializes repeated docking requests so an older placement cannot race a newer one', async () => {
    let finish!: () => void;
    let pending = 0;
    call.mockImplementation(async (command, args) => {
      if (command === 'result_layout') return { frameId: 'test-frame', controlsAbove: false, imageAlignLeft: false };
      if (command === 'set_result_toolbar') {
        if (++pending === 1) await new Promise<void>(resolve => finish = resolve);
        return { frameId: 'test-frame', controlsAbove: args.above, imageAlignLeft: false, toolbarAlignLeft: args.alignLeft };
      }
    });
    const view = renderResult();
    const grip = view.getByRole('button', { name: '移动工具栏' });
    await fireEvent.keyDown(grip, { key: 'ArrowUp' });
    await waitFor(() => expect(pending).toBe(1));
    await fireEvent.keyDown(grip, { key: 'ArrowDown' });
    expect(pending).toBe(1);
    finish();
    await waitFor(() => expect(pending).toBe(2));
    expect(call.mock.calls.filter(([command]) => command === 'set_result_toolbar').map(([,args]) => args.above)).toEqual([true, false]);
  });
  it('flushes a short drag before ending when its animation frame has not run', async () => {
    vi.stubGlobal('requestAnimationFrame', () => 1);
    vi.stubGlobal('cancelAnimationFrame', vi.fn());
    const view = renderResult();
    const handle = view.getByRole('button', { name: '调整右边大小' });
    await fireEvent.pointerDown(handle, { button: 0, pointerId: 1 });
    await fireEvent.pointerMove(handle, { pointerId: 1 });
    await fireEvent.pointerUp(handle, { button: 0, pointerId: 1 });
    await waitFor(() => expect(call).toHaveBeenCalledWith('update_result_resize', { finish: true }), { timeout: 2_000 });
    const writes = call.mock.calls.filter(([command]) => command === 'update_result_resize');
    expect(writes.map(([,args]) => args.finish)).toEqual([false,true]);
  });
  it('flushes the last pointer move after an in-flight resize before restoring tools',async()=>{
    const fallback=call.getMockImplementation()!;
    let release!:()=>void,updates=0;
    call.mockImplementation((command,args)=>{
      if(command==='update_result_resize'&&!args.finish&&++updates===1)return new Promise<void>(resolve=>release=resolve);
      return fallback(command,args);
    });
    const view=render(ResultView,{floating:true}),handle=view.getByRole('button',{name:'调整右边大小'});
    await fireEvent.pointerDown(handle,{button:0,pointerId:1});
    await fireEvent.pointerMove(handle,{pointerId:1});
    await waitFor(()=>expect(updates).toBe(1));
    await fireEvent.pointerMove(handle,{pointerId:1});
    await fireEvent.pointerUp(handle,{button:0,pointerId:1});
    expect(call).not.toHaveBeenCalledWith('update_result_resize',{finish:true});
    release();
    await waitFor(()=>expect(call).toHaveBeenCalledWith('update_result_resize',{finish:true}));
    expect(call.mock.calls.filter(([command])=>command==='update_result_resize').map(([,args])=>args.finish)).toEqual([false,false,true]);
  });
  it('keeps painting the final layout while a resize write completes after pointer release', async () => {
    const fallback=call.getMockImplementation()!;
    let release!:()=>void, settle!:()=>void, reads=0;
    const layout={frameId:'test-frame',controlsAbove:false,imageAlignLeft:false,imageWidth:640,imageHeight:400};
    call.mockImplementation((command,args)=>{
      if(command==='result_layout')return ++reads===1?Promise.resolve(layout):new Promise(resolve=>{settle=()=>resolve({...layout,imageWidth:710});});
      if(command==='update_result_resize'&&!args.finish)return new Promise<void>(resolve=>release=resolve);
      return fallback(command,args);
    });
    const view=render(ResultView,{floating:true}),handle=view.getByRole('button',{name:'调整左边大小'});
    await waitFor(()=>expect(events.get('result-layout-changed')?.size).toBe(1));
    await fireEvent.pointerDown(handle,{button:0,pointerId:1});
    await fireEvent.pointerMove(handle,{pointerId:1});
    await waitFor(()=>expect(release).toBeTypeOf('function'));
    await fireEvent.pointerUp(handle,{button:0,pointerId:1});
    broadcast('result-layout-changed',{...layout,imageWidth:710});release();
    try { await waitFor(()=>expect((view.container.querySelector('.floating-canvas-stage') as HTMLElement).style.width).toBe('710px')); }
    finally { await waitFor(()=>expect(settle).toBeTypeOf('function'));settle(); }
    await waitFor(()=>expect(call).toHaveBeenCalledWith('update_result_resize',{finish:true}));
  });
  it('finishes native resizing even if a size update fails',async()=>{
    const fallback=call.getMockImplementation()!;
    call.mockImplementation((command,args)=>command==='update_result_resize'&&!args.finish?Promise.reject(new Error('resize failed')):fallback(command,args));
    const view=render(ResultView,{floating:true}),handle=view.getByRole('button',{name:'调整右边大小'});
    await fireEvent.pointerDown(handle,{button:0,pointerId:1});await fireEvent.pointerMove(handle,{pointerId:1});
    await waitFor(()=>expect(call).toHaveBeenCalledWith('update_result_resize',{finish:true}));
  });
  it('tracks a handle drag through the image resize commands and ends on release', async () => {
    const view = renderResult();
    const handle = view.getByRole('button', { name: '调整右边大小' });
    await fireEvent.pointerDown(handle, { button: 0, pointerId: 1 });
    await waitFor(() => expect(call).toHaveBeenCalledWith('begin_result_resize', { direction: 'East' }));
    await fireEvent.pointerMove(handle, { pointerId: 1 });
    await waitFor(() => expect(call).toHaveBeenCalledWith('update_result_resize', { finish: false }));
    await fireEvent.pointerUp(handle, { button: 0, pointerId: 1 });
    await waitFor(() => expect(call).toHaveBeenCalledWith('update_result_resize', { finish: true }));
    expect(startDragging).not.toHaveBeenCalled();
  });
  it('moves the independent native toolbar and image through separate drag gestures', async () => {
    const view = renderResult();
    await fireEvent.pointerDown(view.getByRole('button', { name: '移动工具栏' }), { button: 0 });
    await fireEvent.pointerDown(view.container.querySelector('.floating-base-img')!, { button: 0 });
    await waitFor(()=>expect(startDragging).toHaveBeenCalledTimes(2));
    expect(call).toHaveBeenCalledWith('begin_toolbar_drag');
    expect(call).toHaveBeenCalledWith('finish_toolbar_drag');
    const tools=document.querySelector('.toolbar-window')!;
    expect(tools.querySelector('img')).toBeNull();
    expect((tools.querySelector('.floating-hud') as HTMLElement).style.transform).toBe('');
  });

  it('keeps and hides the embedded toolbar through a Wayland image drag', async () => {
    app.data!.capabilities.wayland = true;
    app.data!.capabilities.windowPositioning = false;
    const view = render(ResultView, { floating: true });
    await waitFor(() => expect(events.get('result-layout-changed')?.size).toBe(1));
    expect(view.container.querySelector('.floating-shell')?.contains(view.getByRole('toolbar', { name: '截图操作' }))).toBe(true);
    expect(document.querySelector('.toolbar-window')).toBeNull();

    await fireEvent.pointerDown(view.getByAltText('截图原文'), { button: 0 });
    await waitFor(() => expect(call).toHaveBeenCalledWith('begin_result_drag'));
    expect(startDragging).not.toHaveBeenCalled();
    expect(view.container.querySelector('.floating-controls')?.classList.contains('gesture-active')).toBe(true);
    broadcast('result-drag-ended','test-frame');
    await waitFor(() => expect(view.container.querySelector('.floating-controls')?.classList.contains('gesture-active')).toBe(false));
  });

  it('clamps embedded Wayland toolbar drag offsets inside the result window so the pill never clips', async () => {
    app.data!.capabilities.wayland = true;
    app.data!.capabilities.windowPositioning = false;
    const view = render(ResultView, { floating: true });
    await waitFor(() => expect(events.get('result-layout-changed')?.size).toBe(1));
    const shell = view.container.querySelector('.floating-shell') as HTMLElement;
    const hud = view.container.querySelector('.floating-hud') as HTMLElement;
    const grip = view.getByRole('button', { name: '移动工具栏' });
    vi.spyOn(shell, 'getBoundingClientRect').mockReturnValue({ left: 0, top: 0, right: 256, bottom: 160, width: 256, height: 160, x: 0, y: 0, toJSON() {} });
    vi.spyOn(hud, 'getBoundingClientRect').mockReturnValue({ left: 60, top: 108, right: 248, bottom: 148, width: 188, height: 40, x: 60, y: 108, toJSON() {} });
    await fireEvent.pointerDown(grip, { button: 0, pointerId: 1, clientX: 70, clientY: 128 });
    // Dragging downward/rightward past the bottom-right dock stays clamped at 0px.
    await fireEvent.pointerMove(grip, { pointerId: 1, clientX: 220, clientY: 260 });
    expect(hud.style.transform).toBe('translate(0px, 0px)');
    // Dragging upward/leftward stays within the image height and horizontal dock range.
    await fireEvent.pointerMove(grip, { pointerId: 1, clientX: -120, clientY: -120 });
    expect(hud.style.transform).toBe('translate(-52px, -96px)');
    await fireEvent.pointerUp(grip, { button: 0, pointerId: 1, clientX: -120, clientY: -120 });
  });

  it('starts the next Wayland drag even when the compositor swallowed the previous release', async () => {
    app.data!.capabilities.wayland = true;
    app.data!.capabilities.windowPositioning = false;
    const view = render(ResultView, { floating: true });
    const image = view.getByAltText('截图原文');
    await waitFor(() => expect(events.get('result-drag-ended')?.size).toBe(1));

    await fireEvent.pointerDown(image, { button: 0, pointerId: 1 });
    await waitFor(() => expect(call.mock.calls.filter(([command]) => command === 'begin_result_drag')).toHaveLength(1));
    expect(view.container.querySelector('.floating-controls')?.classList.contains('gesture-active')).toBe(true);

    // No pointerup or result-drag-ended event here: native Wayland move grabs can
    // consume both. The next physical press is enough proof that the old gesture ended.
    await fireEvent.pointerDown(image, { button: 0, pointerId: 1 });
    await waitFor(() => expect(call.mock.calls.filter(([command]) => command === 'begin_result_drag')).toHaveLength(2));
    expect(view.container.querySelector('.floating-controls')?.classList.contains('gesture-active')).toBe(true);

    broadcast('result-drag-ended','test-frame');
    await waitFor(() => expect(view.container.querySelector('.floating-controls')?.classList.contains('gesture-active')).toBe(false));
  });

  it('uses native resize dragging on Wayland instead of pointer deltas', async () => {
    app.data!.capabilities.wayland = true;
    app.data!.capabilities.windowPositioning = false;
    const view = render(ResultView, { floating: true });
    const handle = view.getByRole('button', { name: '调整右边大小' });
    await fireEvent.pointerDown(handle, { button: 0, pointerId: 1, screenX: 100, screenY: 100 });
    await waitFor(() => expect(startResizeDragging).toHaveBeenCalledWith('East'));
    expect(call).toHaveBeenCalledWith('begin_result_resize', { direction: 'East' });
    await fireEvent.pointerMove(handle, { pointerId: 1, screenX: 124, screenY: 136 });
    expect(call).not.toHaveBeenCalledWith('update_result_resize', expect.objectContaining({finish:false}));
    expect(view.container.querySelector('.floating-controls')?.classList.contains('gesture-active')).toBe(true);
    broadcast('result-resize-ended','test-frame');
    await waitFor(() => expect(view.container.querySelector('.floating-controls')?.classList.contains('gesture-active')).toBe(false));
  });

  it('uses independent freely-draggable toolbar and native resize on Linux XWayland', async () => {
    app.data!.capabilities.platform = 'linux';
    app.data!.capabilities.wayland = true;
    app.data!.capabilities.windowPositioning = true;
    const view = renderResult();
    await waitFor(() => expect(events.get('result-resize-ended')?.size).toBe(2));
    // Independent toolbar window is used instead of embedding into `.floating-shell`.
    expect(view.container.querySelector('.floating-shell')?.contains(view.getByRole('toolbar', { name: '截图操作' }))).toBe(false);

    await fireEvent.pointerDown(view.getByRole('button', { name: '移动工具栏' }), { button: 0 });
    await waitFor(() => expect(call).toHaveBeenCalledWith('begin_toolbar_drag'));
    expect(startDragging).not.toHaveBeenCalled();

    await fireEvent.pointerDown(view.getByAltText('截图原文'), { button: 0 });
    await waitFor(() => expect(call).toHaveBeenCalledWith('begin_result_drag'));

    const handle = view.getByRole('button', { name: '调整右下角大小' });
    await fireEvent.pointerDown(handle, { button: 0, pointerId: 1 });
    await waitFor(() => expect(startResizeDragging).toHaveBeenCalledWith('SouthEast'));
    expect(call).toHaveBeenCalledWith('begin_result_resize', { direction: 'SouthEast' });
    broadcast('result-resize-ended', 'test-frame');
    await waitFor(() => expect(view.container.querySelector('.floating-controls')?.classList.contains('gesture-active')).toBe(false));
  });

  it('hides the floating result on Escape even when a toolbar button or resize handle has focus', async () => {
    app.data!.visual.output = 'translation';
    app.data!.visual.job.stage = 'complete';
    const view = renderResult();
    const copyButton = view.getByRole('button', { name: '复制译图' });
    copyButton.focus();
    await fireEvent.keyDown(copyButton, { key: 'Escape' });
    expect(call).toHaveBeenCalledWith('hide_window');

    call.mockClear();
    const handle = view.getByRole('button', { name: '调整右边大小' });
    handle.focus();
    await fireEvent.keyDown(handle, { key: 'Escape' });
    expect(call).toHaveBeenCalledWith('hide_window');
  });

  it('lets the user cancel a running job without starting a window drag', async () => {
    const view = renderResult();
    const button = view.getByRole('button', { name: '取消截图翻译' });
    await fireEvent.pointerDown(button, { button: 0 });
    await fireEvent.click(button);
    expect(startDragging).not.toHaveBeenCalled();
    expect(call).toHaveBeenCalledWith('cancel_job', { session: 'visual' });
    app.data!.visual.job.stage = 'cancelled';
    await tick();
    expect(view.queryByRole('button', { name: '取消截图翻译' })).toBeNull();
    expect(view.getByText('已取消')).toBeTruthy();
  });

  it('replaces the spinner with an actionable error when recognition times out', async () => {
    const view = renderResult();
    app.data!.visual.job = { ...app.data!.visual.job, stage: 'error', error: { code: 'ocr_unavailable', message: '文字识别超时', action: null } };
    await tick();
    expect(view.queryByRole('status')).toBeNull();
    expect(view.getByRole('alert').textContent).toContain('处理超时');
    await fireEvent.click(view.getByRole('button', { name: '重新翻译' }));
    expect(call).toHaveBeenCalledWith('retry_visual');
  });

  it('keeps translation clicks in the overlay and puts the controls outside the image', async () => {
    app.data!.visual.job.stage = 'complete';
    app.data!.visual.frame!.cleanId = 'clean';
    app.data!.visual.blocks = [{ id: 'a', bounds: { x: 10, y: 20, width: 120, height: 30 }, sourceLines: [], text: 'Hello', translation: '你好', targetLanguage: 'zh-Hans', confidence: null, revision: 0, backgroundColor: null, foregroundColor: null, fontSize: 16, lineHeight: 1.28 }];
    const view = renderResult();
    await fireEvent.load(view.getByAltText('清理后的截图'));
    await fireEvent.pointerDown(view.getByText('你好'), { button: 0 });
    await waitFor(()=>expect(startDragging).toHaveBeenCalledTimes(1));
    expect(view.queryByRole('dialog')).toBeNull();
    expect(call.mock.calls.some(([command]) => command === 'navigate_main')).toBe(false);
    const image = view.container.querySelector('.floating-canvas-stage')!;
    expect(image.contains(view.getByRole('toolbar', { name: '截图操作' }))).toBe(false);
    expect(view.getByRole('button', { name: '移动工具栏' })).toBeTruthy();
    await fireEvent.click(view.getByRole('button', { name: '切换原文' }));
    expect((view.container.querySelector('.floating-translation-layer') as HTMLElement).style.visibility).toBe('hidden');
    app.data!.visual.frame = { ...app.data!.visual.frame!, id: 'next-capture' };
    await tick();
    expect(view.getByText('你好')).toBeTruthy();
  });

  it('waits for image decoding before revealing the native window', async () => {
    const view = renderResult();
    await waitFor(() => expect(call).toHaveBeenCalledWith('result_layout'));
    expect(call.mock.calls.some(([command]) => command === 'window_ready')).toBe(false);
    await fireEvent.load(view.getByAltText('截图原文'));
    await waitFor(() => expect(call).toHaveBeenCalledWith('window_ready', { frameId: 'test-frame' }));
  });

  it('does not paint translations over the original before the clean image loads', async () => {
    app.data!.visual.frame!.cleanId = 'clean';
    const view = renderResult();
    await waitFor(()=>expect(call).toHaveBeenCalledWith('result_layout'));
    const layer = view.container.querySelector('.floating-translation-layer') as HTMLElement;
    expect(layer.style.visibility).toBe('hidden');
    await fireEvent.load(view.getByAltText('清理后的截图'));
    expect(layer.style.visibility).toBe('visible');
    app.data!.visual.job.stage = 'complete';
    await tick();
    await fireEvent.click(view.getByRole('button', { name: '切换原文' }));
    await fireEvent.keyDown(window, { key: ' ' });
    await fireEvent.keyUp(window, { key: ' ' });
    expect(view.getByRole('button', { name: '切换原文' }).getAttribute('aria-pressed')).toBe('true');
    expect(layer.style.visibility).toBe('hidden');
  });
});

describe('proofreading dialog', () => {
  function withBlock() {
    const bounds = { x: 10, y: 10, width: 200, height: 40 };
    app.data!.visual.job.stage = 'complete';
    app.data!.visual.blocks = [{ id: 'block-0', bounds, sourceLines: [bounds], text: 'Hello world', translation: '你好，世界', targetLanguage: 'zh-Hans', confidence: null, revision: 0, backgroundColor: null, foregroundColor: null, fontSize: null, lineHeight: null }];
  }

  it('keeps edit and translation status in the card headers and omits the redundant toolbar edit button', async () => {
    withBlock();
    app.data!.visual.frame!.cleanId = 'clean-frame';
    app.data!.visual.input = 'Hello world';
    app.data!.visual.output = '你好，世界';
    call.mockImplementation(async (command) => {
      if (command === 'save_image') return true;
      return undefined;
    });
    const view = render(ResultView);
    expect(view.queryByTitle('校对文字 (E)')).toBeNull();
    expect(view.getByRole('button', { name: '另存为' })).toBeTruthy();

    await fireEvent.click(view.getByRole('button', { name: '复制译图' }));
    await waitFor(() => expect(call).toHaveBeenCalledWith('copy_image', { png: [137, 80, 78, 71] }));
    await waitFor(() => expect(view.getByRole('button', { name: '复制译图' }).querySelector('.copy-done')).not.toBeNull());

    await fireEvent.click(view.getByRole('button', { name: '复制文字' }));
    expect(view.getAllByRole('menuitem').map(node => node.textContent?.trim())).toEqual(['原文', '译文']);
    await fireEvent.click(view.getByRole('menuitem', { name: '译文' }));
    await waitFor(() => expect(call).toHaveBeenCalledWith('copy_text', { text: '你好，世界' }));
    await waitFor(() => expect(view.getByRole('button', { name: '复制文字' }).querySelector('.copy-done')).not.toBeNull());

    await fireEvent.click(view.getByRole('button', { name: '另存为' }));
    await waitFor(() => expect(call).toHaveBeenCalledWith('save_image', { png: [137, 80, 78, 71], defaultName: 'ScreenTranslate.png' }));
    await waitFor(() => expect(view.getByRole('button', { name: '另存为' }).querySelector('.copy-done')).not.toBeNull());

    await fireEvent.load(view.getByAltText('截图与翻译结果'));
    await fireEvent.click(view.getByRole('button', { name: '你好，世界' }));
    const dialog = await view.findByRole('dialog');
    const cards = dialog.querySelectorAll('.correction-card');
    expect(cards).toHaveLength(2);
    expect(dialog.querySelector('.correction-chip')).toBeNull();
    expect(cards[1].querySelector('.copy-icon-well')?.getAttribute('aria-label')).toBe('复制当前块译文');

    await fireEvent.input(view.getByRole('textbox', { name: '识别原文' }), { target: { value: 'Hello there' } });
    expect([...dialog.querySelectorAll('.correction-chip')].map(chip => chip.textContent)).toEqual(['已修改', '待更新']);

    await fireEvent.click(view.getByRole('button', { name: /重译此块/ }));
    expect(call).toHaveBeenCalledWith('edit_block', { id: 'block-0', text: 'Hello there', target: 'zh-Hans' });
    app.data!.visual.blocks = [{ ...app.data!.visual.blocks[0], text: 'Hello there', revision: 1 }];
    app.data!.visual.job.stage = 'translating';
    await tick();
    expect(cards[1].querySelector('.correction-chip')?.textContent).toBe('翻译中');
    expect(cards[1].classList.contains('updating')).toBe(true);
    // The old bottom status line changed the dialog's rows and made it jump.
    expect(dialog.querySelector('.editor-pending')).toBeNull();
  });
});
