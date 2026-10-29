// jsdom does not expose desktop media/geometry observers used by Bits UI and Svelte motion.
import { vi } from 'vitest';
Object.defineProperty(window, 'matchMedia', { writable:true, value:vi.fn().mockImplementation(query => ({
  matches:false, media:query, onchange:null,
  addListener:vi.fn(), removeListener:vi.fn(), addEventListener:vi.fn(), removeEventListener:vi.fn(), dispatchEvent:vi.fn(),
})) });
globalThis.ResizeObserver = class {
  observe() {} unobserve() {} disconnect() {}
};
window.requestAnimationFrame = callback => window.setTimeout(() => callback(performance.now()), 0);
window.cancelAnimationFrame = id => window.clearTimeout(id);
HTMLElement.prototype.scrollIntoView = vi.fn();
