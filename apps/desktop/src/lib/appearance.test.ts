import { describe, expect, it } from 'vitest';
import { actionPalette, contrastRatio } from './appearance';

describe('theme and custom accent contrast', () => {
  it.each([false, true])('keeps a legible action and focus for custom colours (dark=%s)', dark => {
    for (const accent of ['#ffffff', '#000000', '#f0ce8c', '#438569', '#4276b8', '#777777', '#b8442d']) {
      const palette = actionPalette(accent, dark);
      expect(contrastRatio(palette.primary, palette.foreground)).toBeGreaterThanOrEqual(4.5);
      expect(contrastRatio(palette.focus, dark ? '#1a1a1a' : '#ffffff')).toBeGreaterThanOrEqual(3);
      expect(contrastRatio(palette.focus, dark ? '#2a282e' : '#fbf9fc')).toBeGreaterThanOrEqual(3);
    }
  });
  it('recomputes foreground across theme and accent changes', () => {
    expect(actionPalette('#18181b', false).foreground).toBe('#ffffff');
    expect(actionPalette('#18181b', true).foreground).toBe('#000000');
    expect(actionPalette('#f0ce8c', false).foreground).toBe('#000000');
    expect(actionPalette('#000000', true).foreground).toBe('#ffffff');
  });
});
