import { beforeEach, describe, expect, it, vi } from 'vitest';
import { previewBootstrap } from './preview';

vi.mock('./api', () => ({ desktop: false, call: vi.fn(), loadBootstrap: vi.fn() }));
vi.mock('svelte-sonner', () => ({ toast: { error: vi.fn(), success: vi.fn(), warning: vi.fn() } }));
import { app, applySession, editText, registerFlush, flushPendingEdits, recover } from './store.svelte';

beforeEach(() => { app.data = previewBootstrap(); app.input = ''; app.source = 'auto'; app.target = 'auto'; });

describe('cross-window session synchronization', () => {
  it('restores shared input, selected language, and output from a newer session', () => {
    applySession({ ...app.data!.text, revision: 4, input: 'hello', output: '你好', target: 'zh-Hans' });
    expect(app.input).toBe('hello');
    expect(app.target).toBe('zh-Hans');
    expect(app.data!.text.output).toBe('你好');
  });

  it('cannot replace a newer result with an old snapshot', () => {
    applySession({ ...app.data!.text, revision: 8, input: 'new input', output: 'new result' });
    applySession({ ...app.data!.text, revision: 3, input: 'old input', output: 'old result' });
    expect(app.input).toBe('new input');
    expect(app.data!.text.output).toBe('new result');
  });

  it('editing clears the previous result and does not fake a browser translation', () => {
    app.data!.text.output = 'previous result';
    editText('different text', 'en', 'ja', true);
    expect(app.input).toBe('different text');
    expect(app.data!.text.output).toBe('');
    expect(app.data!.text.job.stage).toBe('idle');
  });

  it('visual updates cannot overwrite the independent text session', () => {
    app.input = 'keep this';
    applySession({ ...app.data!.visual, revision: 9, output: 'visual output' });
    expect(app.input).toBe('keep this');
    expect(app.data!.visual.output).toBe('visual output');
  });
});

describe('saving before quit', () => {
  it('blurs the focused field so it queues its edit, then runs every registered flush', async () => {
    const order: string[] = [];
    const input = document.createElement('input');
    input.addEventListener('blur', () => order.push('blur'));
    document.body.append(input);
    input.focus();
    const stop = registerFlush(async () => { order.push('flush'); });
    const removed = vi.fn(async () => {});
    registerFlush(removed)();
    await flushPendingEdits();
    expect(order).toEqual(['blur', 'flush']);
    expect(removed).not.toHaveBeenCalled();
    stop();
    input.remove();
  });

  it('still finishes when a flush fails', async () => {
    const stop = registerFlush(() => Promise.reject(new Error('disk full')));
    await expect(flushPendingEdits()).resolves.toBeUndefined();
    stop();
  });

  it('routes hotkey recovery actions to the shortcuts page', () => {
    app.route = 'translate';
    recover('hotkeys');
    expect(app.route).toBe('shortcuts');
  });
});
