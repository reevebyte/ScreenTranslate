import { browser, $, expect } from '@wdio/globals';
import { mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';
import { readFileSync } from 'node:fs';

const version=JSON.parse(readFileSync(new URL('../../package.json',import.meta.url),'utf8')).version;
const directory=resolve(`../../build/release-${version}/native-services`);
async function settleSelection(){
  await browser.waitUntil(()=>browser.execute(()=>{
    const p=document.querySelector('.service-list');
    const a=p.querySelector('.active-indicator').getBoundingClientRect(),b=p.querySelector('[aria-pressed="true"]').getBoundingClientRect();
    return Math.abs(a.y-b.y)<.25&&Math.abs(a.height-b.height)<.25;
  }));
}
describe('service configuration through native IPC',()=>{
  before(async()=>{await mkdir(directory,{recursive:true});});
  it('searches local models and keeps connection feedback in a fixed row',async()=>{
    await $('[aria-label="设置"]').click();await $('[role="tab"]=外观').click();await $('button=浅色').click();
    expect(await browser.execute(()=>document.documentElement.classList.contains('dark'))).toBe(false);
    await $('[aria-label="服务中心"]').click();
    await $('[role="tab"]=翻译接口').click();
    await $('[aria-label="OpenAI 兼容"]').click();
    expect(await browser.execute(()=>document.querySelectorAll('.service-list-group').length)).toBe(2);
    expect(await browser.execute(()=>document.querySelectorAll('.service-monogram').length)).toBe(0);
    await $('[aria-label="选择模型"]').click();
    await $('div=fixture-vision').waitForDisplayed();
    expect(await browser.execute(()=>document.querySelectorAll('.model-option .provider-icon').length)).toBeGreaterThan(0);
    await browser.saveScreenshot(resolve(directory,'model-menu-light.png'));
    await $('[aria-label="搜索模型"]').setValue('vision');
    await browser.keys(['ArrowDown','Enter']);
    await expect($('#provider-model')).toHaveValue('fixture-vision');
    const before=await $('.service-detail-actions').getLocation();
    await $('button=测试连接').click();
    await $('[aria-label^="连接成功："]').waitForDisplayed();
    const after=await $('.service-detail-actions').getLocation();
    expect(before).toEqual(after);
    await settleSelection();await browser.saveScreenshot(resolve(directory,'services-light.png'));
    await $('#provider-model').setValue('manually-entered');
    await expect($('#provider-model')).toHaveValue('manually-entered');
    await browser.waitUntil(async()=>!await $('[aria-label^="连接成功："]').isExisting());
    await $('[aria-label="设置"]').click();await $('[role="tab"]=外观').click();await $('button=深色').click();
    await $('[aria-label="服务中心"]').click();await $('[aria-label="OpenAI 兼容"]').click();
    await expect($('#provider-model')).toHaveValue('manually-entered');
    await settleSelection();await browser.saveScreenshot(resolve(directory,'services-dark.png'));
  });
  it('keeps the service picker and fixed actions usable at 600 by 400 with 150 percent type',async()=>{
    await $('[aria-label="设置"]').click();await $('[role="tab"]=外观').click();
    for(let i=0;i<10;i++)await $('[aria-label="增大界面字号"]').click();
    await browser.execute(async()=>{const a=window.__TAURI__;await a.window.getCurrentWindow().setSize(new a.dpi.LogicalSize(600,400));});
    await $('[aria-label="服务中心"]').click();
    await $('[aria-label="翻译服务"]').waitForDisplayed();
    const geometry=await browser.execute(()=>{const n=document.querySelector('.service-detail-actions').getBoundingClientRect(),b=document.querySelector('.service-detail-body');return {right:n.right,bottom:n.bottom,overflow:b.scrollWidth>b.clientWidth,body:b.clientHeight};});
    expect(geometry.right).toBeLessThanOrEqual(600);expect(geometry.bottom).toBeLessThanOrEqual(400);expect(geometry.overflow).toBe(false);expect(geometry.body).toBeGreaterThan(100);
    await browser.saveScreenshot(resolve(directory,'services-min150.png'));
  });
  it('keeps Bing and Azure gradients local to each rendered icon after selection',async()=>{
    await browser.tauri.execute(async({core})=>{const {config}=await core.invoke('bootstrap');config.appearance.fontScale=1;await core.invoke('save_config',{config});});
    await browser.execute(async()=>{const a=window.__TAURI__;await a.window.getCurrentWindow().setSize(new a.dpi.LogicalSize(780,520));});
    await $('[aria-label="服务中心"]').click();await $('[role="tab"]=翻译接口').click();
    for(let n=0;n<3;n++){await $('[aria-label="Bing 网页翻译"]').click();await $('[aria-label="Google 网页"]').click();}
    await $('[aria-label="Bing 网页翻译"]').click();await settleSelection();await browser.saveScreenshot(resolve(directory,'bing-selected.png'));
    await $('[role="tab"]=文字识别').click();await $('[aria-label="Azure Vision"]').click();await settleSelection();await browser.saveScreenshot(resolve(directory,'azure-selected.png'));
    const references=await browser.execute(()=>{const ids=[...document.querySelectorAll('svg [id]')].map(n=>n.id);const broken=[...document.querySelectorAll('svg [fill^="url(#"]')].filter(n=>!n.closest('svg').querySelector('[id="'+n.getAttribute('fill').slice(5,-1)+'"]'));return {unique:new Set(ids).size===ids.length,broken:broken.length};});expect(references).toEqual({unique:true,broken:0});
  });
  it('keeps manufacturer marks and tone-only editors readable in both themes',async()=>{
    await browser.tauri.execute(async({core})=>{const {config}=await core.invoke('bootstrap');config.appearance.fontScale=1;await core.invoke('save_config',{config});});
    await browser.execute(async()=>{const a=window.__TAURI__;await a.window.getCurrentWindow().setSize(new a.dpi.LogicalSize(780,520));});
    for(const theme of ['light','dark']) {
      await $('[aria-label="设置"]').click();await $('[role="tab"]=外观').click();await $(`button=${theme==='dark'?'深色':'浅色'}`).click();
      expect(await browser.execute(()=>document.documentElement.classList.contains('dark'))).toBe(theme==='dark');
      await $('[aria-label="服务中心"]').click();await $('[role="tab"]=翻译接口').click();await $('[aria-label="DeepL"]').click();await settleSelection();
      expect(await $('.service-detail-heading [data-brand="deepl"]').isExisting()).toBe(true);
      await browser.execute(()=>document.activeElement?.blur());
      await browser.saveScreenshot(resolve(directory,`directory-${theme}.png`));
      await $('[role="tab"]=文字识别').click();await $('[aria-label="有道 OCR"]').click();await settleSelection();
      await browser.waitUntil(()=>browser.execute(()=>{const image=document.querySelector('.service-detail-heading img');return image?.complete&&image.naturalWidth>0;}));
      await browser.execute(()=>document.activeElement?.blur());
      await browser.saveScreenshot(resolve(directory,`ocr-${theme}.png`));
      await $('[aria-label="文字翻译"]').click();
      const style=await browser.execute(()=>{document.activeElement?.blur();const a=document.querySelector('.source-pane'),b=document.querySelector('.target-pane'),c=document.querySelector('.translation-card');return {source:getComputedStyle(a).backgroundColor,target:getComputedStyle(b).backgroundColor,border:getComputedStyle(c).borderTopWidth};});
      expect(style.source).not.toBe(style.target);expect(style.border).toBe('0px');
      await browser.saveScreenshot(resolve(directory,`text-${theme}.png`));
    }
  });
});
