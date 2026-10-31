import { browser, $, expect } from '@wdio/globals';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const version = JSON.parse(await readFile(new URL('../../package.json', import.meta.url), 'utf8')).version;
const directory = resolve('../../build/release-' + version + '/native-soft-ui');
async function appearance(theme, fontScale = 1, size = [780, 520]) {
  await browser.tauri.execute(async ({ core }, appearance) => {
    const { config } = await core.invoke('bootstrap');
    Object.assign(config.appearance, appearance);
    config.autoUpdate = false;
    await core.invoke('save_config', { config });
  }, { theme, fontScale });
  await browser.execute(async size => {
    const a = window.__TAURI__;
    await a.window.getCurrentWindow().setSize(new a.dpi.LogicalSize(...size));
  }, size);
}
async function settle() {
  await browser.execute(async () => {
    for (let i = 0; i < 18; i++) await new Promise(requestAnimationFrame);
  });
}

describe('soft desktop design through native windows', () => {
  before(async () => { await mkdir(directory, { recursive: true }); });
  it('shows all centered About actions at the normal window size in both themes', async () => {
    for (const theme of ['light', 'dark']) {
      await appearance(theme);
      await $('[aria-label="设置"]').click();
      await $('[role="tab"]=关于').click();
      await settle();
      const bounds = await browser.execute(() => {
        const panel = document.querySelector('.about-panel'), list = document.querySelector('.about-resources');
        return { bottom: list.getBoundingClientRect().bottom, available: panel.getBoundingClientRect().bottom, overflow: panel.scrollWidth > panel.clientWidth };
      });
      expect(bounds.bottom).toBeLessThanOrEqual(bounds.available + 1);
      expect(bounds.overflow).toBe(false);
      await browser.saveScreenshot(resolve(directory, 'about-' + theme + '.png'));
      await $('[role="tab"]=常规').click();
      await settle();
      await browser.saveScreenshot(resolve(directory, 'settings-' + theme + '.png'));
      await $('[aria-label="快捷操作"]').click();
      await settle();
      await browser.saveScreenshot(resolve(directory, 'shortcuts-' + theme + '.png'));
    }
  });
  it('keeps hidden preferences and every About action reachable with 150 percent text', async () => {
    await appearance('light', 1.5, [600, 400]);
    await $('[aria-label="设置"]').click();
    await $('[role="tab"]=常规').click();
    await $('summary=更多设置').click();
    await $('#auto-copy').scrollIntoView();
    await expect($('#auto-copy')).toBeDisplayed();
    await browser.saveScreenshot(resolve(directory, 'advanced-min150.png'));
    await $('[role="tab"]=关于').click();
    await $('button=使用指南').scrollIntoView();
    const dimensions = await browser.execute(() => {
      const panel = document.querySelector('.about-panel');
      return { overflow: panel.scrollWidth > panel.clientWidth, scrolled: panel.scrollTop };
    });
    expect(dimensions.overflow).toBe(false);
    expect(dimensions.scrolled).toBeGreaterThan(0);
    await browser.saveScreenshot(resolve(directory, 'about-min150.png'));
    await $('button=使用指南').click();
    await $('.welcome-dialog').waitForDisplayed();
    await browser.saveScreenshot(resolve(directory, 'guide-min150.png'));
    await $('[aria-label="稍后配置"]').click();
  });
  it('preserves English labels and stable content origin during module switches', async () => {
    await appearance('dark', 1, [780, 520]);
    await browser.execute(async () => {
      localStorage.setItem('screentranslate-ui-language', 'en');
      await window.__TAURI__.event.emit('ui-locale-changed', { language: 'en' });
    });
    await $('[aria-label="Settings"]').waitForDisplayed();
    await $('[role="tab"]=About').click();
    await settle();
    await expect($('label=Check for updates automatically')).toBeDisplayed();
    await browser.saveScreenshot(resolve(directory, 'about-english.png'));
    const positions = [];
    for (const label of ['Shortcuts', 'Translate', 'Services', 'Settings']) {
      await $('[aria-label="' + label + '"]').click();
      positions.push(await browser.execute(() => {
        const node = document.querySelector('.page-body').getBoundingClientRect();
        return { x: node.x, y: node.y };
      }));
    }
    expect(positions.every(p => p.x === positions[0].x && p.y === positions[0].y)).toBe(true);
    await writeFile(resolve(directory, 'module-origin.json'), JSON.stringify(positions, null, 2));
  });
  it('scrolls service choices only on wheel or keyboard input and keeps the OCR icon round', async () => {
    await appearance('dark', 1, [600, 400]);
    await $('[aria-label="Services"]').click();
    await $('[role="tab"]=Translation').click();
    await $('.service-mobile [role="combobox"]').click();
    const viewport = $('.select-viewport');
    await viewport.waitForDisplayed();
    expect(await $('[data-slot="select-scroll-up-button"]').isExisting()).toBe(false);
    expect(await $('[data-slot="select-scroll-down-button"]').isExisting()).toBe(false);
    const initial = await browser.execute(() => {
      const menu = document.querySelector('.select-viewport');
      menu.scrollTop = 0;
      const box = menu.getBoundingClientRect();
      return { x: Math.round(box.x + box.width / 2), y: Math.round(box.bottom - 4), overflow: menu.scrollHeight > menu.clientHeight };
    });
    expect(initial.overflow).toBe(true);
    await browser.performActions([{ type: 'pointer', id: 'menu-mouse', parameters: { pointerType: 'mouse' }, actions: [{ type: 'pointerMove', duration: 100, x: initial.x, y: initial.y }, { type: 'pause', duration: 450 }] }]);
    expect(await browser.execute(() => document.querySelector('.select-viewport').scrollTop)).toBe(0);
    await browser.performActions([{ type: 'wheel', id: 'menu-wheel', actions: [{ type: 'scroll', x: initial.x, y: initial.y, deltaX: 0, deltaY: 240, duration: 150 }] }]);
    await browser.waitUntil(() => browser.execute(() => document.querySelector('.select-viewport').scrollTop > 0));
    await browser.saveScreenshot(resolve(directory, 'service-wheel-menu.png'));
    await browser.keys(['Home', 'ArrowDown', 'Enter']);
    await browser.waitUntil(async () => !await $('.choice-menu').isExisting());
    await $('[role="tab"]=OCR').click();
    await $('.service-mobile [role="combobox"]').click();
    await $('[role="option"]=Youdao OCR').click();
    const icon = await browser.execute(() => {
      const icon = document.querySelector('.connection-line > .setting-row-icon').getBoundingClientRect();
      return { width: icon.width, height: icon.height };
    });
    expect(icon.width).toBe(30);
    expect(icon.height).toBe(30);
    await browser.saveScreenshot(resolve(directory, 'ocr-round-icon.png'));
  });
});
