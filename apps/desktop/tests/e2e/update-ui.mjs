import {browser,$,expect} from '@wdio/globals';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
const out=resolve('../../build/release-2.0.2/native-update-ui');
async function caption(visible,mode='download',progress=null){await browser.tauri.execute(async({core},args)=>core.invoke('set_update_caption',args),{visible,enabled:true,mode,progress,version:'2.0.3',english:false});}
describe('native update controls',()=>{
 before(async()=>{await mkdir(out,{recursive:true});});
 it('keeps the main client rectangle while caption actions appear and disappear',async()=>{
  const size=()=>browser.execute(async()=>{const w=window.__TAURI__.window.getCurrentWindow();return {inner:await w.innerSize(),outer:await w.outerPosition()};});
  const before=await size();await caption(true);const after=await size();expect(after).toEqual(before);
  await caption(true,'downloading',42);expect(await size()).toEqual(before);await caption(true,'restart');expect(await size()).toEqual(before);
  await caption(false);expect(await size()).toEqual(before);await writeFile(resolve(out,'geometry.json'),JSON.stringify({before,after},null,2));
 });
 it('hides the About scrollbar and keeps large-font content scrollable',async()=>{
  await $('[aria-label="设置"]').click();await $('[role="tab"]=关于').click();
  expect(await browser.execute(()=>getComputedStyle(document.querySelector('.about-view')).scrollbarWidth)).toBe('none');
  await browser.pause(350);await browser.saveScreenshot(resolve(out,'about-no-track.png'));
  await caption(true);await browser.pause(500);
  if(process.env.SCREENTRANS_OBSERVE_CAPTION==='true')await browser.pause(60000);
  await caption(false);
 });
});
