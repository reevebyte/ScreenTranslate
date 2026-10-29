import { afterEach, describe, expect, it, vi } from 'vitest';
import { createAutosave } from './autosave';

afterEach(() => vi.useRealTimers());
describe('settings autosave', () => {
  it('coalesces rapid edits and flushes when leaving a page', async () => {
    vi.useFakeTimers();
    const write = vi.fn(async (_value: string) => true);
    const saver = createAutosave(write);
    saver.schedule('first');
    saver.schedule('latest');
    await vi.advanceTimersByTimeAsync(249);
    expect(write).not.toHaveBeenCalled();
    await saver.flush();
    expect(write.mock.calls).toEqual([['latest']]);
    await vi.runAllTimersAsync();
    expect(write).toHaveBeenCalledTimes(1);
  });
  it('serializes edits made during an unfinished write', async () => {
    let complete!: (saved: boolean) => void;
    const write = vi.fn().mockImplementationOnce(() => new Promise<boolean>(resolve => complete = resolve)).mockResolvedValue(true);
    const saver = createAutosave<string>(write);
    saver.schedule('first');
    const saving = saver.flush();
    await Promise.resolve();
    saver.schedule('intermediate');
    saver.schedule('latest');
    const leaving = saver.flush();
    expect(write.mock.calls).toEqual([['first']]);
    complete(true);
    await Promise.all([saving, leaving]);
    expect(write.mock.calls).toEqual([['first'], ['latest']]);
  });
  it('keeps failed edits for explicit retry without looping', async () => {
    vi.useFakeTimers();
    const write = vi.fn().mockRejectedValueOnce(new Error('disk unavailable')).mockResolvedValue(true);
    const saver = createAutosave<string>(write);
    saver.schedule('keep this');
    expect(await saver.flush()).toBe(false);
    await vi.runAllTimersAsync();
    expect(write).toHaveBeenCalledTimes(1);
    expect(await saver.retry()).toBe(true);
    expect(write.mock.calls).toEqual([['keep this'], ['keep this']]);
  });
});
