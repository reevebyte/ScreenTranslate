import { browser, $, $$, expect } from '@wdio/globals';
import { mkdir, writeFile, access, unlink } from 'node:fs/promises';
import { resolve } from 'node:path';
import { readFileSync } from 'node:fs';

const version = JSON.parse(readFileSync(new URL('../../package.json',import.meta.url),'utf8')).version;
const directory = resolve(`../../build/release-${version}/native-layout`);
let main, image, tools;
async function mode(name) {
  for (const handle of await browser.getWindowHandles()) {
    await browser.switchToWindow(handle);
    if (await browser.execute(() => new URLSearchParams(location.search).get('window') || 'main') === name) return handle;
  }
  return null;
}
async function geometry() {
  return browser.execute(async () => {
    const w=window.__TAURI__.window.getCurrentWindow();
    const p=await w.innerPosition(),s=await w.innerSize();
    const n=document.querySelector('.floating-canvas-stage');
    const r=n?.getBoundingClientRect();
    return {window:{x:p.x,y:p.y,width:s.width,height:s.height},image:r?{x:p.x+r.x*devicePixelRatio,y:p.y+r.y*devicePixelRatio,width:r.width*devicePixelRatio,height:r.height*devicePixelRatio}:null,visible:await w.isVisible(),dpr:devicePixelRatio};
  });
}
async function settle() { await browser.execute(async()=>{
  for(let i=0;i<12;i++)await new Promise(requestAnimationFrame);
  await Promise.all(document.getAnimations().filter(a=>a.playState==='running'&&a.effect?.getTiming().iterations!==Infinity).map(a=>a.finished.catch(()=>{})));
}); }

describe('native result windows and appearance', () => {
  before(async()=>{main=await browser.getWindowHandle();await mkdir(directory,{recursive:true});if(process.env.SCREENTRANS_MANUAL_QA)await unlink(resolve(directory,'qa-finished')).catch(()=>{});});
  it('keeps the same settings node and switches every selected-navigation colour atomically',async()=>{
    await $('[aria-label="设置"]').click();
    await $('[role="tab"]=外观').click();
    await settle();
    await browser.execute(()=>{window.settingsNode=document.querySelector('.settings-layout');});
    for(const theme of ['dark','light','dark','light']) {
      const samples=await browser.execute(async theme=>{
        const root=document.documentElement;
        const name=theme==='dark'?'深色':'浅色';
        [...document.querySelectorAll('button')].find(n=>n.textContent.trim()===name).click();
        const samples=[];
        for(let i=0;i<20;i++) {
          await new Promise(requestAnimationFrame);
          const node=document.querySelector('.rail-link[aria-current="page"]');
          samples.push({same:window.settingsNode===document.querySelector('.settings-layout'),dark:root.classList.contains('dark'),background:getComputedStyle(node).backgroundColor,surface:getComputedStyle(root).getPropertyValue('--surface').trim(),classes:root.className,transition:getComputedStyle(node).transition});
        }
        return samples;
      },theme);
      await writeFile(resolve(directory,`theme-${theme}.json`),JSON.stringify(samples,null,2));
      await browser.saveScreenshot(resolve(directory,`settings-${theme}.png`));
      expect(samples.every(s=>s.same&&s.dark===(theme==='dark'))).toBe(true);
      expect(new Set(samples.map(s=>s.background)).size).toBe(1);
    }
    await $('[aria-label="文字翻译"]').click();
    await settle();
    await browser.execute(()=>document.activeElement?.blur());
    await browser.saveScreenshot(resolve(directory,'main.png'));
  });
  it('presents one welcome action with keyboard focus and clear OCR location in both themes',async()=>{
    for(const theme of ['light','dark']) {
      await $('[aria-label="设置"]').click();await $('[role="tab"]=外观').click();
      await $(`button=${theme==='dark'?'深色':'浅色'}`).click();
      await $('[role="tab"]=关于').click();await $('button=使用指南').click();await settle();
      await expect($('.welcome-dialog')).toBeDisplayed();
      await expect($('.welcome-privacy')).toHaveText(expect.stringContaining('本机识别'));
      expect(await $$('.welcome-actions button').length).toBe(1);
      expect(await browser.execute(()=>{const n=document.querySelector('.welcome-dialog');return n.scrollWidth<=n.clientWidth;})).toBe(true);
      await expect($('[aria-label="原文与译文分界"]')).toHaveValue('50');
      expect(await browser.execute(()=>document.querySelector('.welcome-dialog').getBoundingClientRect().width)).toBe(360);
      expect(await browser.execute(()=>document.querySelector('.welcome-dialog').textContent.includes('读懂屏幕上的每一句'))).toBe(false);
      await browser.keys('Tab');
      await browser.saveScreenshot(resolve(directory,`welcome-${theme}.png`));
      for(const value of [0,100]) {
        await browser.execute(value=>{const s=document.querySelector('[aria-label="原文与译文分界"]');s.value=String(value);s.dispatchEvent(new Event('input',{bubbles:true}));},value);
        await expect($('[aria-label="原文与译文分界"]')).toHaveValue(String(value));
        await browser.waitUntil(async()=>await browser.execute(value=>document.querySelector('.welcome-comparison-divider').style.left===`${value}%`,value));
        await settle();await browser.saveScreenshot(resolve(directory,`welcome-${theme}-${value===0?'english':'chinese'}.png`));
      }
      const focus=await browser.execute(()=>{const n=document.activeElement;return{inside:document.querySelector('.welcome-dialog').contains(n),button:n?.tagName==='BUTTON',visible:n?.matches(':focus-visible'),element:n?.outerHTML};});
      await writeFile(resolve(directory,`welcome-focus-${theme}.json`),JSON.stringify(focus,null,2));
      expect(focus.inside).toBe(true);
      await browser.keys('Escape');await expect($('.welcome-dialog')).not.toExist();
    }
    await $('[role="tab"]=外观').click();await $('button=浅色').click();
    for(let i=0;i<10;i++)await $('[aria-label="增大界面字号"]').click();
    await browser.execute(async()=>{const a=window.__TAURI__;await a.window.getCurrentWindow().setSize(new a.dpi.LogicalSize(600,400));});
    await $('[role="tab"]=关于').click();await $('button=使用指南').click();await settle();
    expect(await browser.execute(()=>{const n=document.querySelector('.welcome-dialog'),b=n.getBoundingClientRect();return n.scrollWidth<=n.clientWidth&&b.height<=innerHeight-31&&b.top>=15;})).toBe(true);
    await browser.saveScreenshot(resolve(directory,'welcome-min150.png'));
    await browser.keys('Escape');await $('[role="tab"]=外观').click();
    for(let i=0;i<10;i++)await $('[aria-label="减小界面字号"]').click();
    await browser.execute(async()=>{const a=window.__TAURI__;await a.window.getCurrentWindow().setSize(new a.dpi.LogicalSize(780,520));});
    await $('[aria-label="文字翻译"]').click();
  });
  it('opens separate visible image and toolbar windows through real OCR and localhost translation',async()=>{
    await browser.tauri.execute(({core},path)=>core.invoke('open_image',{fixturePath:path}),resolve('../../crates/platform/tests/fixtures/ocr-fixture.png'));
    await browser.waitUntil(async()=>{image=await mode('result');tools=await mode('result-tools');return !!image&&!!tools;});
    await browser.switchToWindow(image);
    await $('.floating-canvas-stage').waitForDisplayed();
    await browser.waitUntil(async()=>await browser.tauri.execute(async({core})=>(await core.invoke('bootstrap')).visual.job.stage==='complete'),{timeout:45000});
    await settle();
    expect((await geometry()).visible).toBe(true);
    expect(await browser.execute(()=>getComputedStyle(document.querySelector('.result-selection-border')).outlineWidth)).toBe('2px');
    await browser.saveScreenshot(resolve(directory,'result.png'));
    await browser.switchToWindow(tools);
    await $('.floating-hud').waitForDisplayed();
    await browser.waitUntil(async()=>(await geometry()).visible);
    expect(await $('img').isExisting()).toBe(false);
    await browser.saveScreenshot(resolve(directory,'toolbar.png'));
  });
  it('keeps native source and translation copy feedback independent without touching the system clipboard',async()=>{
    await browser.switchToWindow(tools);
    // The e2e-only Rust branch validates the request without writing the host clipboard.
    await $('[aria-label="提取文字"]').click();
    await browser.waitUntil(async()=>await $('[aria-label="提取文字"] .copy-done').isExisting());
    await expect($('[aria-label="复制全部译文"] .copy-done')).not.toExist();
    await browser.saveScreenshot(resolve(directory,'copy-source.png'));
    await $('[aria-label="复制全部译文"]').click();
    await expect($('[aria-label="复制全部译文"] .copy-done')).toExist();
    await expect($('[aria-label="提取文字"] .copy-done')).toExist();
  });
  it('keeps minimum-size main layouts usable with 150 percent text and long input',async()=>{
    await browser.switchToWindow(main);
    await $('[aria-label="设置"]').click();await $('[role="tab"]=外观').click();
    for(let i=0;i<10;i++)await $('[aria-label="增大界面字号"]').click();
    await expect($('output[aria-label="界面字号"]')).toHaveText('150%');
    await browser.execute(async()=>{const a=window.__TAURI__;await a.window.getCurrentWindow().setSize(new a.dpi.LogicalSize(600,400));});
    await $('[aria-label="文字翻译"]').click();
    await $('[aria-label="待翻译的文字"]').setValue('Long text with 中文内容。'.repeat(30));
    await settle();
    await browser.saveScreenshot(resolve(directory,'main-min150.png'));
    const dimensions=await browser.execute(()=>{const b=document.querySelector('[aria-label="待翻译的文字"]').getBoundingClientRect();return{width:b.width,height:b.height,bodyWidth:document.body.scrollWidth,viewportWidth:innerWidth};});
    await writeFile(resolve(directory,'main-min150.json'),JSON.stringify(dimensions,null,2));
    expect(dimensions.width>100&&dimensions.height>50&&dimensions.bodyWidth<=dimensions.viewportWidth).toBe(true);
    await $('[aria-label="设置"]').click();await $('[role="tab"]=外观').click();await settle();
    expect(await browser.execute(()=>document.body.scrollWidth<=innerWidth)).toBe(true);
    await browser.saveScreenshot(resolve(directory,'settings-min150.png'));
    for(let i=0;i<10;i++)await $('[aria-label="减小界面字号"]').click();
    await browser.execute(async()=>{const a=window.__TAURI__;await a.window.getCurrentWindow().setSize(new a.dpi.LogicalSize(780,520));});
  });
  it('changes only the dragged axis and preserves opposite anchors on all eight handles',async()=>{
    await browser.switchToWindow(image);
    const evidence=await browser.execute(async()=>{
      const a=window.__TAURI__,w=a.window.getCurrentWindow(),cursor=await a.window.cursorPosition(),evidence=[];
      async function snap(){for(let i=0;i<4;i++)await new Promise(requestAnimationFrame);const p=await w.innerPosition(),b=document.querySelector('.floating-canvas-stage').getBoundingClientRect();return{x:p.x+b.x*devicePixelRatio,y:p.y+b.y*devicePixelRatio,width:b.width*devicePixelRatio,height:b.height*devicePixelRatio};}
      try {
        for(const [direction,h,v] of [['East',1,0],['West',-1,0],['North',0,-1],['South',0,1],['NorthWest',-1,-1],['NorthEast',1,-1],['SouthEast',1,1],['SouthWest',-1,1]]) {
          const before=await snap();let p=await w.innerPosition();
          const start={x:before.x+before.width*(h+1)/2,y:before.y+before.height*(v+1)/2};
          await w.setCursorPosition(new a.dpi.PhysicalPosition(Math.round(start.x-p.x),Math.round(start.y-p.y)));
          await a.core.invoke('begin_result_resize',{direction});
          p=await w.innerPosition();
          await w.setCursorPosition(new a.dpi.PhysicalPosition(Math.round(start.x+h*24-p.x),Math.round(start.y+v*12-p.y)));
          const backend=await a.core.invoke('update_result_resize',{finish:false});
          await a.core.invoke('update_result_resize',{finish:true});
          evidence.push({direction,h,v,before,backend,after:await snap()});
        }
      } finally {const p=await w.innerPosition();await w.setCursorPosition(new a.dpi.PhysicalPosition(cursor.x-p.x,cursor.y-p.y));}
      return evidence;
    });
    await writeFile(resolve(directory,'resize.json'),JSON.stringify(evidence,null,2));
    for(const {before,after,backend,h,v} of evidence) {
      expect(Math.abs(after.width-backend.width)).toBeLessThan(2);
      expect(Math.abs(after.height-backend.height)).toBeLessThan(2);
      if(!h)expect(Math.abs(after.width-before.width)).toBeLessThan(.5);
      if(!v)expect(Math.abs(after.height-before.height)).toBeLessThan(.5);
      expect(Math.abs(after.x+after.width*(1-h)/2-before.x-before.width*(1-h)/2)).toBeLessThan(2);
      expect(Math.abs(after.y+after.height*(1-v)/2-before.y-before.height*(1-v)/2)).toBeLessThan(2);
    }
    await browser.saveScreenshot(resolve(directory,'stretched.png'));
  });
  it('keeps a detached toolbar visible beyond the image bounds',async()=>{
    await browser.switchToWindow(image);const before=await geometry();
    await browser.switchToWindow(tools);
    await browser.execute(async target=>{const a=window.__TAURI__,w=a.window.getCurrentWindow();await a.core.invoke('begin_toolbar_drag');await w.setPosition(new a.dpi.PhysicalPosition(target.x,target.y));await a.core.invoke('finish_toolbar_drag');}, {x:before.window.x+40,y:Math.max(50,before.window.y-100)});
    await settle();
    const after=await geometry();expect(after.visible).toBe(true);
    expect(after.window.y+after.window.height).toBeLessThan(before.window.y);
    await expect($('.floating-hud')).toBeDisplayed();
    await browser.saveScreenshot(resolve(directory,'detached-toolbar.png'));
    await browser.switchToWindow(image);const unmoved=await geometry();
    expect(unmoved.image).toEqual(before.image);
    await writeFile(resolve(directory,'detached.json'),JSON.stringify({before,after,unmoved},null,2));
  });
  it('automatically avoids every work-area corner without moving image content',async()=>{
    await browser.switchToWindow(tools);
    await browser.execute(()=>window.__TAURI__.core.invoke('set_result_toolbar',{above:false,alignLeft:false}));
    await browser.switchToWindow(image);
    const evidence=[];
    for(const corner of ['top-left','top-right','bottom-left','bottom-right']) {
      await browser.execute(async corner=>{const a=window.__TAURI__,w=a.window.getCurrentWindow(),m=await a.window.currentMonitor(),s=await w.innerSize(),r=m.workArea;await w.setPosition(new a.dpi.PhysicalPosition(corner.endsWith('left')?r.position.x:r.position.x+r.size.width-s.width,corner.startsWith('top')?r.position.y:r.position.y+r.size.height-s.height));},corner);
      await settle();const picture=await geometry();
      await browser.switchToWindow(tools);
      await browser.waitUntil(async()=>{const t=await geometry();return t.visible;});await settle();
      const toolbar=await geometry();
      const area=await browser.execute(async()=> (await window.__TAURI__.window.currentMonitor()).workArea);
      expect(toolbar.window.x).toBeGreaterThanOrEqual(area.position.x);
      expect(toolbar.window.y).toBeGreaterThanOrEqual(area.position.y);
      expect(toolbar.window.x+toolbar.window.width).toBeLessThanOrEqual(area.position.x+area.size.width);
      expect(toolbar.window.y+toolbar.window.height).toBeLessThanOrEqual(area.position.y+area.size.height);
      evidence.push({corner,picture,toolbar,area});
      await browser.switchToWindow(image);
    }
    await writeFile(resolve(directory,'corners.json'),JSON.stringify(evidence,null,2));
  });
  it('returns below the image after leaving the bottom edge instead of remembering automatic avoidance',async()=>{
    await browser.switchToWindow(tools);await browser.execute(()=>window.__TAURI__.core.invoke('set_result_toolbar',{above:false,alignLeft:false}));
    await browser.switchToWindow(image);
    await browser.execute(async()=>{
      const a=window.__TAURI__,w=a.window.getCurrentWindow(),m=await a.window.currentMonitor(),r=document.querySelector('.floating-canvas-stage').getBoundingClientRect();
      await a.core.invoke('begin_result_drag');await w.setPosition(new a.dpi.PhysicalPosition(m.workArea.position.x+300,Math.round(m.workArea.position.y+m.workArea.size.height-(r.y+r.height)*devicePixelRatio-2)));await a.core.invoke('finish_result_drag');
    });
    await settle();const edgeImage=await geometry();
    await browser.switchToWindow(tools);await browser.waitUntil(async()=>(await geometry()).visible);const above=await geometry();
    expect(above.window.y+above.window.height).toBeLessThanOrEqual(edgeImage.image.y);
    await browser.switchToWindow(image);
    await browser.execute(async()=>{const a=window.__TAURI__,w=a.window.getCurrentWindow(),m=await a.window.currentMonitor();await a.core.invoke('begin_result_drag');await w.setPosition(new a.dpi.PhysicalPosition(m.workArea.position.x+300,m.workArea.position.y+80));await a.core.invoke('finish_result_drag');});
    await settle();const centeredImage=await geometry();
    await browser.switchToWindow(tools);await browser.waitUntil(async()=>(await geometry()).visible);const below=await geometry();
    expect(below.window.y).toBeGreaterThanOrEqual(centeredImage.image.y+centeredImage.image.height);
    await writeFile(resolve(directory,'edge-return.json'),JSON.stringify({edgeImage,above,centeredImage,below},null,2));
    await browser.saveScreenshot(resolve(directory,'toolbar-return.png'));
  });
  it('hides detached tools throughout an image move, keeps the selection border and docks after release',async()=>{
    await browser.switchToWindow(tools);
    await browser.execute(async()=>{const a=window.__TAURI__;await a.core.invoke('begin_toolbar_drag');await a.window.getCurrentWindow().setPosition(new a.dpi.PhysicalPosition(100,80));await a.core.invoke('finish_toolbar_drag');});
    await browser.switchToWindow(image);
    const before=await geometry();
    await browser.execute(()=>window.__TAURI__.core.invoke('begin_result_drag'));
    await browser.switchToWindow(tools);
    expect((await geometry()).visible).toBe(false);
    await browser.switchToWindow(image);
    const border=await browser.execute(()=>{const s=getComputedStyle(document.querySelector('.result-selection-border'));return{visibility:s.visibility,outline:s.outlineWidth,anchors:document.querySelectorAll('.resize-handle').length};});
    expect(border).toEqual({visibility:'visible',outline:'2px',anchors:8});
    await browser.execute(async()=>{const a=window.__TAURI__;await a.window.getCurrentWindow().setPosition(new a.dpi.PhysicalPosition(320,180));});
    await settle();await browser.saveScreenshot(resolve(directory,'moving-border.png'));
    await browser.switchToWindow(tools);expect((await geometry()).visible).toBe(false);
    await browser.switchToWindow(image);await browser.execute(()=>window.__TAURI__.core.invoke('finish_result_drag'));
    await browser.switchToWindow(tools);await browser.waitUntil(async()=>(await geometry()).visible);
    expect(await browser.execute(async()=> (await window.__TAURI__.core.invoke('toolbar_placement')).detached)).toBe(false);
    await writeFile(resolve(directory,'move-recovery.json'),JSON.stringify({before,border,toolbar:await geometry()},null,2));
  });
  it('keeps native viewport geometry and glyph layout stable during a continuous resize',async()=>{
    await browser.switchToWindow(image);
    const samples=await browser.execute(async()=>{
      const a=window.__TAURI__,w=a.window.getCurrentWindow(),cursor=await a.window.cursorPosition();
      const start=await w.innerPosition();
      const node=document.querySelector('.floating-block');
      const font=node.style.fontSize,blockWidth=node.style.width,samples=[];
      await w.setCursorPosition(new a.dpi.PhysicalPosition(400,150));
      try {
        await a.core.invoke('begin_result_resize',{direction:'East'});
        for(let step=1;step<=12;step++) {
          await w.setCursorPosition(new a.dpi.PhysicalPosition(400+step*6,150));
          await a.core.invoke('update_result_resize',{finish:false});
          await new Promise(requestAnimationFrame);
          const stage=document.querySelector('.floating-canvas-stage').getBoundingClientRect();
          const picture=document.querySelector('.floating-base-img').getBoundingClientRect();
          samples.push({width:stage.width,viewport:innerWidth,picture:picture.width,font:node.style.fontSize,blockWidth:node.style.width,fontExpected:font,blockWidthExpected:blockWidth});
        }
        await a.core.invoke('update_result_resize',{finish:true});
      } finally {const p=await w.innerPosition();await w.setCursorPosition(new a.dpi.PhysicalPosition(cursor.x-p.x,cursor.y-p.y));}
      return samples;
    });
    await writeFile(resolve(directory,'continuous-resize.json'),JSON.stringify(samples,null,2));
    for(const sample of samples) {
      expect(Math.abs(sample.width+16-sample.viewport)).toBeLessThan(.6);
      expect(Math.abs(sample.picture-sample.width)).toBeLessThan(.6);
      expect(sample.font).toBe(sample.fontExpected);
      expect(sample.blockWidth).toBe(sample.blockWidthExpected);
    }
    await browser.saveScreenshot(resolve(directory,'resize-stable.png'));
  });
  it('synchronizes view controls and hides or restores both native windows together',async()=>{
    await browser.switchToWindow(tools);await $('[aria-label="切换原文"]').click();
    await browser.switchToWindow(image);await browser.waitUntil(async()=> await $('.floating-translation-layer').getCSSProperty('visibility').then(r=>r.value==='hidden'));
    await browser.switchToWindow(tools);await $('[aria-label="收起截图结果"]').click();
    await browser.waitUntil(async()=> !(await geometry()).visible);
    await browser.switchToWindow(image);expect((await geometry()).visible).toBe(false);
    await browser.switchToWindow(main);await browser.tauri.execute(({core})=>core.invoke('toggle_result'));
    await browser.switchToWindow(image);await browser.waitUntil(async()=>(await geometry()).visible);
    await browser.switchToWindow(tools);await browser.waitUntil(async()=>(await geometry()).visible);
  });
  it('preserves tiny capture dimensions and restores its tools after compact feedback',async()=>{
    await browser.switchToWindow(main);
    const png=await browser.execute(()=>{const c=document.createElement('canvas');c.width=80;c.height=24;const x=c.getContext('2d');x.fillStyle='#fff';x.fillRect(0,0,80,24);x.fillStyle='#222';x.font='14px sans-serif';x.fillText('Hi',8,17);return c.toDataURL('image/png').split(',')[1];});
    const fixture=resolve(directory,'tiny-fixture.png');await writeFile(fixture,Buffer.from(png,'base64'));
    await browser.tauri.execute(({core},path)=>core.invoke('open_image',{fixturePath:path}),fixture);
    await browser.switchToWindow(image);
    await browser.waitUntil(async()=>await browser.execute(async()=>{const s=await window.__TAURI__.core.invoke('bootstrap');return s.visual.frame?.width===80&&!['recognizing','translating'].includes(s.visual.job.stage);}));
    await settle();const before=await geometry();
    expect(before.image.width).toBe(80);expect(before.image.height).toBe(24);
    await browser.execute(()=>window.__TAURI__.core.invoke('show_result_notice',{text:'识别文字已复制',tone:'normal'}));
    await $('[role="status"]').waitForDisplayed();await settle();
    await browser.saveScreenshot(resolve(directory,'tiny-feedback.png'));
    const after=await geometry();expect(after.image).toEqual(before.image);
    await browser.switchToWindow(tools);expect((await geometry()).visible).toBe(false);
    await browser.waitUntil(async()=>(await geometry()).visible,{timeout:5000});
    await browser.switchToWindow(image);expect((await geometry()).image).toEqual(before.image);
  });
  it('opens two-column correction with a complete inset focus and no annotation tools',async()=>{
    await browser.switchToWindow(main);
    await browser.tauri.execute(({core},path)=>core.invoke('open_image',{fixturePath:path}),resolve('../../crates/platform/tests/fixtures/ocr-fixture.png'));
    await browser.waitUntil(async()=>await browser.tauri.execute(async({core})=>(await core.invoke('bootstrap')).visual.job.stage==='complete'),{timeout:45000});
    await $('[aria-label="截图结果"]').click();await $('.image-canvas').waitForDisplayed();await settle();
    await expect($('[aria-label="马赛克"]')).not.toExist();await expect($('[aria-label="添加文字"]')).not.toExist();
    await $('.translated-block').click();await $('.editor-dialog').waitForDisplayed();await $('#block-source').click();await settle();
    const columns=await browser.execute(()=>{const a=document.querySelector('#block-source').getBoundingClientRect(),b=document.querySelector('.block-translation').getBoundingClientRect();return{x:a.x,right:a.right,targetX:b.x,height:a.height,targetHeight:b.height};});expect(columns.targetX).toBeGreaterThan(columns.right);expect(Math.abs(columns.height-columns.targetHeight)).toBeLessThan(1);
    await browser.saveScreenshot(resolve(directory,'correction-columns.png'));await $('#block-language').click();await $('.choice-menu').waitForDisplayed();await settle();
    expect(await browser.execute(()=>{const m=document.querySelector('.choice-menu'),b=m.getBoundingClientRect();return +getComputedStyle(m).zIndex>51&&b.y>=0&&b.bottom<=innerHeight&&!!document.elementFromPoint(b.x+20,b.y+20)?.closest('.choice-menu');})).toBe(true);
    await browser.saveScreenshot(resolve(directory,'correction-menu.png'));await browser.keys('Escape');await $('[aria-label="关闭校对"]').click();
    await $('[aria-label="文字翻译"]').click();expect(await browser.execute(()=>!!document.querySelector('.workspace-toolbar .translation-submit')&&!document.querySelector('.workspace-footer')&&!document.querySelector('.engine-chip'))).toBe(true);
    await $('[aria-label="快捷操作"]').click();const spacing=await browser.execute(()=>{const a=document.querySelector('.shortcut-cluster').getBoundingClientRect(),b=document.querySelector('.shortcut-workspace').getBoundingClientRect();return{top:a.y-b.y,bottom:b.bottom-a.bottom};});expect(spacing.top).toBeGreaterThan(0);expect(spacing.bottom).toBeGreaterThan(spacing.top);expect(spacing.bottom-spacing.top).toBeLessThan(33);await browser.saveScreenshot(resolve(directory,'home-centered.png'));
  });
  it('runs the custom HTTP OCR pipeline with optional credentials in the isolated vault',async()=>{
    await browser.switchToWindow(main);
    await browser.tauri.execute(async({core})=>{const s=await core.invoke('bootstrap');s.config.ocr.engine='custom';await core.invoke('save_secret',{name:'custom_ocr',secret:'fixture-ocr',persistent:false});await core.invoke('save_config',{config:s.config});});
    await browser.tauri.execute(({core},path)=>core.invoke('open_image',{fixturePath:path}),resolve('../../crates/platform/tests/fixtures/ocr-fixture.png'));
    await browser.waitUntil(async()=>await browser.tauri.execute(async({core})=>(await core.invoke('bootstrap')).visual.job.stage==='complete'),{timeout:45000});
    const result=await browser.tauri.execute(async({core})=>{const s=await core.invoke('bootstrap');return{input:s.visual.input,output:s.visual.output,secret:s.credentials.custom_ocr,bounds:s.visual.blocks[0].bounds};});
    expect(result.input).toContain('Custom OCR fixture');expect(result.output).toContain('本地测试');expect(result.secret).toBe(true);expect(result.bounds.width).toBeGreaterThan(0);
    await writeFile(resolve(directory,'custom-ocr.json'),JSON.stringify(result,null,2));
  });
  after(async function(){
    this.timeout(300000);
    if (!process.env.SCREENTRANS_MANUAL_QA) return;
    await browser.switchToWindow(image);
    await browser.execute(async()=>{const a=window.__TAURI__;await a.window.getCurrentWindow().setPosition(new a.dpi.PhysicalPosition(300,160));});
    await browser.switchToWindow(tools);
    await browser.execute(()=>window.__TAURI__.core.invoke('set_result_toolbar',{above:false,alignLeft:false}));
    console.log('MANUAL_QA_READY');
    const end=Date.now()+240000;
    while(Date.now()<end) {
      if(await access(resolve(directory,'qa-finished')).then(()=>true,()=>false)) break;
      await new Promise(resolve=>setTimeout(resolve,1000));
    }
    await browser.saveScreenshot(resolve(directory,'manual-toolbar.png'));
    await writeFile(resolve(directory,'manual-final-geometry.json'),JSON.stringify(await geometry(),null,2));
  });
});
