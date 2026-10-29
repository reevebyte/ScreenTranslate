import { browser, expect } from '@wdio/globals';
import { mkdir,writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { readFileSync } from 'node:fs';
import { spawn,execFile } from 'node:child_process';
import { promisify } from 'node:util';
const version=JSON.parse(readFileSync(new URL('../../package.json',import.meta.url),'utf8')).version;
const directory=resolve('../../build/release-'+version+'/resize-'+(process.env.SCREENTRANS_RESIZE_PROBE||'after'));
async function findImage(){for(const handle of await browser.getWindowHandles()){await browser.switchToWindow(handle);if(await browser.execute(()=>new URLSearchParams(location.search).get('window'))==='result')return;}throw Error('No result window');}
describe('continuous resize frame alignment',()=>{
 it('samples physical image edges on every compositor frame for narrow and wide captures',async()=>{
  await mkdir(directory,{recursive:true});const main=await browser.getWindowHandle();const results=[];
  for(const width of [80,320]) {
   await browser.switchToWindow(main);
   const png=await browser.execute(width=>{const c=document.createElement('canvas');c.width=width;c.height=120;const x=c.getContext('2d');x.fillStyle='#fff';x.fillRect(0,0,width,120);x.fillStyle='#222';x.font='20px sans-serif';x.fillText('Hello',8,40);return c.toDataURL().split(',')[1];},width);
   const fixture=resolve(directory,`fixture-${width}.png`);await writeFile(fixture,Buffer.from(png,'base64'));await browser.tauri.execute(({core},path)=>core.invoke('open_image',{fixturePath:path}),fixture);
   await findImage();await browser.waitUntil(async()=>await browser.execute(async width=>{const s=await window.__TAURI__.core.invoke('bootstrap');return s.visual.frame?.width===width&&!['recognizing','translating'].includes(s.visual.job.stage);},width));
   for(const direction of ['East','West','NorthWest']) {
    let pixelProcess,pixelOutput='',pixelDone;
    if(process.env.SCREENTRANS_PIXEL_PYTHON) {
      const anchor=await browser.execute(direction=>{const r=document.querySelector('.floating-canvas-stage').getBoundingClientRect();return{x:(screenX+(direction==='East'?r.left:r.right))*devicePixelRatio,y:(screenY+r.bottom-16)*devicePixelRatio,inside:direction==='East'?'after':'before'};},direction);
      pixelProcess=spawn(process.env.SCREENTRANS_PIXEL_PYTHON,[resolve('./tests/resize-border-probe.py'),JSON.stringify(anchor)],{windowsHide:true});
      pixelDone=new Promise((resolve,reject)=>{pixelProcess.once('error',reject);pixelProcess.once('close',resolve);});
      await new Promise((resolve,reject)=>{pixelProcess.stdout.on('data',data=>{pixelOutput+=data.toString();if(pixelOutput.includes('ready'))resolve();});pixelProcess.once('error',reject);});
    }
    await browser.saveScreenshot(resolve(directory,`native-webview-${width}-${direction}-start.png`));
    const samples=await browser.execute(async direction=>{
     const a=window.__TAURI__,w=a.window.getCurrentWindow(),dpr=devicePixelRatio,cursor=await a.window.cursorPosition();
     await w.setCursorPosition(new a.dpi.PhysicalPosition(80,40));const edge={East:'e',West:'w',NorthWest:'nw'}[direction],handle=document.querySelector('.resize-handle.'+edge);const fire=type=>handle.dispatchEvent(new PointerEvent(type,{bubbles:true,pointerId:99,button:0}));fire('pointerdown');for(let i=0;i<3;i++)await new Promise(requestAnimationFrame);
     const origin=await a.window.cursorPosition(),samples=[];let alive=true;
     const sample=()=>{const r=document.querySelector('.floating-canvas-stage').getBoundingClientRect();samples.push({x:(screenX+r.x)*dpr,y:(screenY+r.y)*dpr,w:r.width*dpr,h:r.height*dpr,viewport:innerWidth*dpr,native:document.querySelector('.floating-canvas-stage').classList.contains('native-resizing')});if(alive)requestAnimationFrame(sample);};sample();
     try {for(let n=1;n<=24;n++){const p=await w.innerPosition();await w.setCursorPosition(new a.dpi.PhysicalPosition(origin.x+n*3-p.x,origin.y+(direction==='NorthWest'?n:0)-p.y));fire('pointermove');await new Promise(requestAnimationFrame);await new Promise(requestAnimationFrame);}fire('pointerup');for(let n=0;n<30 && document.querySelector('.floating-canvas-stage').classList.contains('native-resizing');n++)await new Promise(requestAnimationFrame);for(let n=0;n<4;n++)await new Promise(requestAnimationFrame);}finally{alive=false;const p=await w.innerPosition();await w.setCursorPosition(new a.dpi.PhysicalPosition(cursor.x-p.x,cursor.y-p.y));}
     return samples;
    },direction);
    await browser.saveScreenshot(resolve(directory,`native-webview-${width}-${direction}-end.png`));
    const fixed=direction==='East'?samples.map(s=>s.x):samples.map(s=>s.x+s.w);const drift=Math.max(...fixed)-Math.min(...fixed);
    let pixels=null;
    if(pixelProcess){pixelProcess.stdin.end('stop\n');await pixelDone;pixels=JSON.parse(pixelOutput.trim().split('\n').at(-1));}
    results.push({width,direction,drift,samples,pixels});
   }
  }
  await writeFile(resolve(directory,'frames.json'),JSON.stringify(results,null,2));console.log('RESIZE_METRICS',JSON.stringify(results.map(({samples,pixels,...r})=>({...r,pixels:pixels?{drift:pixels.drift,visible:pixels.visible,missing:pixels.missing,imageMissing:pixels.imageMissing,maxImageGap:pixels.maxImageGap,maxWidth:pixels.maxWidth}:null}))));
  if(process.env.SCREENTRANS_RESIZE_PROBE!=='before')for(const r of results){const visibleCss=r.samples.filter(s=>!s.native);for(const s of visibleCss){const fixed=r.direction==='East'?s.x:s.x+s.w,initial=r.direction==='East'?r.samples[0].x:r.samples[0].x+r.samples[0].w;expect(Math.abs(fixed-initial)).toBeLessThanOrEqual(1.1);}if(r.pixels){expect(r.pixels.drift).toBeLessThanOrEqual(1);expect(r.pixels.missing).toBe(0);expect(r.pixels.visible).toBeGreaterThan(5);expect(r.pixels.imageMissing).toBe(0);}}
  if(process.env.SCREENTRANS_PIXEL_PYTHON){const {stdout}=await promisify(execFile)(process.env.SCREENTRANS_PIXEL_PYTHON,[resolve('./tests/resize-border-probe.py'),'count'],{windowsHide:true});expect(Number(stdout.trim())).toBe(0);}
 });
 it('settles all eight handles through rapid reversals without a release jump',async()=>{
  await findImage();
  const results=await browser.execute(async()=>{
   const a=window.__TAURI__,w=a.window.getCurrentWindow(),dpr=devicePixelRatio;
   const cursor=await a.window.cursorPosition(),results=[];
   const frame=()=>new Promise(requestAnimationFrame);
   const rect=()=>{const r=document.querySelector('.floating-canvas-stage').getBoundingClientRect();return{x:(screenX+r.left)*dpr,y:(screenY+r.top)*dpr,right:(screenX+r.right)*dpr,bottom:(screenY+r.bottom)*dpr,width:r.width*dpr,height:r.height*dpr};};
   try {for(const direction of ['e','w','n','s','ne','nw','se','sw']) {
    const initial=rect(),handle=document.querySelector('.resize-handle.'+direction),r=handle.getBoundingClientRect();
    await w.setCursorPosition(new a.dpi.PhysicalPosition(Math.round((r.left+r.width/2)*dpr),Math.round((r.top+r.height/2)*dpr)));
    const origin=await a.window.cursorPosition(),fire=type=>handle.dispatchEvent(new PointerEvent(type,{bubbles:true,pointerId:100,button:0}));
    fire('pointerdown');await frame();await frame();
    for(const delta of [3,12,6,-6,-12,0,9,18,0]) {
     const p=await w.innerPosition();await w.setCursorPosition(new a.dpi.PhysicalPosition(origin.x+(direction.includes('e')||direction.includes('w')?delta:0)-p.x,origin.y+(direction.includes('n')||direction.includes('s')?delta:0)-p.y));
     fire('pointermove');await frame();
    }
    fire('pointerup');const release=[];
    for(let n=0;n<40;n++) {await frame();release.push(rect());if(n>6 && !document.querySelector('.floating-canvas-stage').classList.contains('native-resizing'))break;}
    const final=rect(),fixedX=direction.includes('w')?'right':'x',fixedY=direction.includes('n')?'bottom':'y';
    results.push({direction,initial,final,release,fixedX,fixedY});
   }}finally{const p=await w.innerPosition();await w.setCursorPosition(new a.dpi.PhysicalPosition(cursor.x-p.x,cursor.y-p.y));}
   return results;
  });
  await writeFile(resolve(directory,'rapid-reversals.json'),JSON.stringify(results,null,2));
  for(const result of results) {
   for(const sample of result.release) {
    expect(Math.abs(sample[result.fixedX]-result.initial[result.fixedX])).toBeLessThanOrEqual(1.1);
    expect(Math.abs(sample[result.fixedY]-result.initial[result.fixedY])).toBeLessThanOrEqual(1.1);
   }
  }
 });
});
