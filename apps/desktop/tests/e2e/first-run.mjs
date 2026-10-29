import { browser, $, expect } from '@wdio/globals';
import { mkdir, readFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const version=JSON.parse(await readFile(new URL('../../package.json',import.meta.url),'utf8')).version;
const directory=resolve('../../build/release-'+version+'/native-first-run');

describe('clean first launch',()=>{
  it('automatically opens the guide with a fresh isolated config, then starts on shortcuts',async()=>{
    await mkdir(directory,{recursive:true});
    const initial=await browser.tauri.execute(({core})=>core.invoke('bootstrap'));
    expect(initial.config.onboardingComplete).toBe(false);
    await $('.welcome-dialog').waitForDisplayed();
    await browser.saveScreenshot(resolve(directory,'first-launch-guide.png'));
    await $('.welcome-actions button').click();
    await browser.waitUntil(async()=>!(await $('.welcome-dialog').isExisting()));
    await expect($('.rail-link[aria-label="快捷操作"], .rail-link[aria-label="Shortcuts"]')).toHaveAttribute('aria-current','page');
    const saved=await browser.tauri.execute(({core})=>core.invoke('bootstrap'));
    expect(saved.config.onboardingComplete).toBe(true);
    await browser.saveScreenshot(resolve(directory,'first-launch-shortcuts.png'));
    // Re-bootstrap and reopen the existing window: completed guidance must stay dismissed.
    await browser.tauri.execute(({core})=>core.invoke('show_main'));
    await expect($('.welcome-dialog')).not.toExist();
  });
});
