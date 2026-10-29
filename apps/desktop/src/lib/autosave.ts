/** Debounce edits, serialize writes, and keep failed values available for retry. */
export function createAutosave<T>(write: (value: T) => Promise<boolean>, delay = 250) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let pending: T | undefined;
  let failed: T | undefined;
  let running: Promise<boolean> | null = null;
  function clearTimer() { if (timer !== undefined) clearTimeout(timer); timer = undefined; }
  async function flush(): Promise<boolean> {
    clearTimer();
    if (running) { await running; return flush(); }
    if (pending === undefined) return failed === undefined;
    const value = pending;
    pending = undefined;
    const flight = Promise.resolve().then(() => write(value)).catch(() => false);
    running = flight;
    const saved = await flight;
    running = null;
    failed = saved ? undefined : value;
    return pending !== undefined ? flush() : saved;
  }
  return {
    schedule(value: T) { pending = value; failed = undefined; clearTimer(); timer = setTimeout(() => { void flush(); }, delay); },
    flush,
    retry() { if (pending === undefined && failed !== undefined) pending = failed; failed = undefined; return flush(); },
  };
}
