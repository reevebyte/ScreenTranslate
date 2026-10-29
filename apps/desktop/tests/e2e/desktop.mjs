import {browser,$,expect} from '@wdio/globals';
import {mkdir,writeFile,readFile} from 'node:fs/promises';
import {resolve} from 'node:path';
const version=JSON.parse(await readFile(new URL('../../package.json',import.meta.url),'utf8')).version;
const directory=resolve('../../build/release-'+version+'/native-desktop');let main;
async function snap(name){await browser.execute(async()=>{await document.fonts.ready;for(let i=0;i<16;i++)await new Promise(requestAnimationFrame);});await browser.saveScreenshot(resolve(directory,name));}
async function mode(name){for(const handle of await browser.getWindowHandles()){await browser.switchToWindow(handle);if(await browser.execute(()=>new URLSearchParams(location.search).get('window')||'main')===name)return handle;}throw Error('Missing window '+name);}
describe('current desktop workflow',()=>{
 before(async()=>{main=await browser.getWindowHandle();await mkdir(directory,{recursive:true});});
 beforeEach(async()=>{await browser.switchToWindow(main);});
 it('translates through Rust IPC using a localhost fixture',async()=>{await $('[aria-label="文字翻译"]').click();await $('[aria-label="待翻译的文字"]').setValue('Hello from ScreenTranslate');await $('button=翻译').click();await expect($('[aria-label="翻译结果"]')).toHaveText('本地测试：Hello from ScreenTranslate');});
 it('shares input and translation with the quick window',async()=>{await browser.tauri.execute(({core})=>core.invoke('toggle_quick'));await mode('quick');await expect($('[aria-label="快速翻译输入"]')).toHaveValue('Hello from ScreenTranslate');await expect($('.spotlight-result-text')).toHaveText(expect.stringContaining('本地测试：Hello from ScreenTranslate'));await snap('quick-zh.png');await $('[aria-label="收起"]').click();});
 it('switches existing pages and the quick window to English without changing user text',async()=>{
  await $('[aria-label="设置"]').click();await $('[role="tab"]=通用').click();await $('[aria-label="界面语言"]').click();await browser.keys(['ArrowDown','Enter']);await $('[role="tab"]=General').waitForDisplayed();expect(await browser.execute(()=>document.documentElement.lang)).toBe('en');
  await $('[aria-label="Translate"]').click();await expect($('[aria-label="Text to translate"]')).toHaveValue('Hello from ScreenTranslate');
  await browser.tauri.execute(({core})=>core.invoke('toggle_quick'));await mode('quick');await $('[aria-label="Quick translation input"]').waitForDisplayed();await snap('quick-en.png');await $('[aria-label="Hide"]').click();await browser.switchToWindow(main);
  await $('[aria-label="Settings"]').click();await $('[role="tab"]=About').click();await snap('about-en.png');
  const raw=await browser.execute(()=>document.querySelector('.settings-panel[data-state="active"]').innerText);expect(/[\u3400-\u9fff]/.test(raw)).toBe(false);
 });
 it('keeps module origins stable on every frame and supports minimum English layouts',async()=>{
  const frames=[];for(const name of ['Services','Shortcuts','Captures','Translate','Settings'])frames.push(...await browser.execute(async name=>{[...document.querySelectorAll('.rail-link')].find(n=>n.getAttribute('aria-label')===name).click();const a=[];for(let i=0;i<12;i++){await new Promise(requestAnimationFrame);const r=document.querySelector('.page-body').getBoundingClientRect();a.push({name,x:r.x,y:r.y});}return a;},name));await writeFile(resolve(directory,'module-frames.json'),JSON.stringify(frames,null,2));for(const name of ['Services','Shortcuts','Captures','Translate','Settings']){const ys=frames.filter(r=>r.name===name).map(r=>r.y);expect(Math.max(...ys)-Math.min(...ys)).toBeLessThanOrEqual(.1);}
  await $('[role="tab"]=Appearance').click();for(let i=0;i<10;i++)await $('[aria-label="Increase text size"]').click();await expect($('output[aria-label="Text size"]')).toHaveText('150%');await browser.execute(async()=>{const a=window.__TAURI__;await a.window.getCurrentWindow().setSize(new a.dpi.LogicalSize(600,400));});await $('[role="tab"]=About').click();expect(await browser.execute(()=>{const n=document.querySelector('.about-view');return n.scrollWidth<=n.clientWidth;})).toBe(true);await snap('about-en-min150.png');await $('[aria-label="Translate"]').click();expect(await browser.execute(()=>{const n=document.querySelector('.page-body');return n.scrollWidth-n.clientWidth;})).toBeLessThanOrEqual(1);await snap('translate-en-min150.png');
 });
 it('persists automatic update preference and starts the guide on shortcuts',async()=>{
  await $('[aria-label="Settings"]').click();await $('[role="tab"]=About').click();
  const preference=await $('#auto-update');await expect(preference).toHaveAttribute('aria-checked','true');await preference.click();
  await browser.waitUntil(async()=>await browser.tauri.execute(async({core})=>(await core.invoke('bootstrap')).config.autoUpdate)===false);
  await $('[aria-label="Translate"]').click();await $('[aria-label="Settings"]').click();await $('[role="tab"]=About').click();await expect($('#auto-update')).toHaveAttribute('aria-checked','false');
  await $('#auto-update').click();await browser.waitUntil(async()=>await browser.tauri.execute(async({core})=>(await core.invoke('bootstrap')).config.autoUpdate)===true);
  await snap('about-auto-update-en-min150.png');
  await $('button=User guide').click();await $('[role="dialog"]').waitForDisplayed();await snap('guide-en-min150.png');await $('button=Get started').click();
  await expect($('[aria-label="Shortcuts"]')).toHaveAttribute('aria-current','page');
  expect(await browser.tauri.execute(async({core})=>(await core.invoke('bootstrap')).config.onboardingComplete)).toBe(true);
 });

});
