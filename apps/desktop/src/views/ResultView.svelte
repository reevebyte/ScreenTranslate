<script lang="ts">
  import CopyButton from '$lib/components/CopyButton.svelte';
  import { t } from '$lib/i18n.svelte';
  import { fade } from 'svelte/transition';
  import { prefersReducedMotion } from 'svelte/motion';
  import { extractedText, screenshotNotice } from '$lib/screenshot-feedback';
  import { Dialog, Tabs } from 'bits-ui';
  import { onMount, onDestroy, tick } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { desktop } from '$lib/api';
  import {
    ScanLine,
    Copy,
    RotateCcw,
    PencilLine,
    X,
    TextScan, Settings2,
    BrandHandle,
    ArrowUpRight,
    LoaderCircle,
    Eye, FolderOpen,
    Check, ArrowRightLeft,
  } from '$lib/icons';
  import { app, act, copy, recover } from '$lib/store.svelte';
  import { call } from '$lib/api';
  import { frameUrl, languageOptions } from '$lib/utils';
  import { fitOverlayText, sourceFontSize } from '$lib/overlay-layout';
  import Button from '$lib/components/ui/button.svelte';
  import Choice from '$lib/components/ui/choice.svelte';

  let { floating = false, toolsOnly = false }: { floating?: boolean; toolsOnly?: boolean } = $props();
  let original = $state(false);
  let holdingOriginal = $state(false);
  let comparePosition = $state(50);
  let cleanReady = $state('');
  let originalReady = $state('');
  let layoutReady = $state(!desktop);
  let imageError = $state(false);
  let showOriginal = $derived(original || holdingOriginal);
  let comparing = $state(false);
  let selectedId = $state('');
  let draftText = $state('');
  let draftTarget = $state('auto');
  let previousKey = '';
  let toolbarAlignLeft = $state(false);
  let toolbarDragging = $state(false);
  let toolbarOffset = $state({ x: 0, y: 0 });
  let notice = $state<{ text: string; tone: 'normal' | 'error' } | null>(null);
  let noticeTimer: ReturnType<typeof setTimeout> | undefined;
  let lastNotice = '';
  let dockWrite: Promise<void> = Promise.resolve();
  let toolbarGesture: { id: number; node: HTMLElement; x: number; y: number } | null = null;
  let workspaceWidth = $state(800);
  let editorTab = $state('source');
  let compactEditor = $derived(workspaceWidth < 470 && (app.data?.config.appearance.fontScale ?? 1) > 1.2);
  let submittedEdit = $state<{id:string;text:string;target:string}|null>(null);
  let copied = $state({ source: false, translation: false });
  const copyTimers: Partial<Record<'source' | 'translation', ReturnType<typeof setTimeout>>> = {};
  onDestroy(() => { clearTimeout(copyTimers.source); clearTimeout(copyTimers.translation); });
  let presented = '';
  let width = $state(800);
  let height = $state(440);
  let imageWidth = $state(0);
  let imageHeight = $state(0);
  let imageDragging = $state(false);
  let controlsAbove = $state(false);
  let imageAlignLeft = $state(false);
  const controlsHeight = 48;
  const imagePadding = 8;
  type ResultLayout = { frameId: string; controlsAbove: boolean; imageAlignLeft: boolean; toolbarAlignLeft?: boolean; imageWidth?: number; imageHeight?: number };
  let resultLayout = $state<ResultLayout | null>(null);
  let disposed = false;
  async function revealReady() {
    if (toolsOnly || !floating || !desktop || !layoutReady || resultLayout?.frameId !== frame?.id || (!imageError && originalReady !== frame?.id)) return;
    const frameId = frame?.id;
    await tick();
    await document.fonts?.ready;
    if (!disposed && frame?.id === frameId && resultLayout?.frameId === frameId && presented !== frameId) {
      presented = frameId || '';
      await call('window_ready', { frameId }).catch(() => {});
      await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      if (!disposed && frame?.id === frameId) void call('result_presented', { frameId }).catch(() => {});
    }
  }
  function originalLoaded(event: Event) {
    originalReady = (event.currentTarget as HTMLImageElement).dataset.frameId || '';
    imageError = false;
    void revealReady();
  }
  function cleanLoaded(event: Event) { cleanReady = (event.currentTarget as HTMLImageElement).dataset.frameId || ''; }
  onMount(() => {
    if (!floating || !desktop) return;
    const stops: (() => void)[] = [];
    async function subscribe<T>(name: string, handler: (payload:T)=>void) {
      const stop = await listen<T>(name, ({payload}) => { if (!disposed) handler(payload); });
      if (disposed) stop(); else stops.push(stop);
    }
    const applyDisplay = (value: {frameId:string;original:boolean;comparing:boolean;holdingOriginal:boolean}) => {
      if (value?.frameId === frame?.id) { original=value.original;comparing=value.comparing;holdingOriginal=value.holdingOriginal; }
    };
    void (async () => {
      if (!toolsOnly) await subscribe<{text:string;tone:'normal'|'error'}>('result-feedback', value => showNotice(value.text,value.tone));
      await subscribe('result-display-changed', applyDisplay);
      applyDisplay(await call('result_display'));
      if (toolsOnly) {
        const apply = (value:{above:boolean;alignLeft:boolean}) => { if(value){controlsAbove=value.above;toolbarAlignLeft=value.alignLeft;} };
        await subscribe('toolbar-placement-changed',apply);
        apply(await call('toolbar_placement'));
      } else {
        const apply = (value:ResultLayout) => { if(value)resultLayout=value; };
        await subscribe('result-layout-changed', apply);
        apply(await call('result_layout'));
      }
    })().catch(()=>{});
    return () => { disposed=true;stops.forEach(stop=>stop()); };
  });

  let session = $derived(app.data?.visual);
  let sourceText = $derived(extractedText(session));
  let frame = $derived(session?.frame);
  $effect(() => {
    if (resultLayout && resultLayout.frameId === frame?.id) {
      controlsAbove = resultLayout.controlsAbove;
      imageAlignLeft = resultLayout.imageAlignLeft;
      toolbarAlignLeft = resultLayout.toolbarAlignLeft ?? false;
      layoutReady = true;
      void revealReady();
    } else if (desktop && floating) layoutReady = false;
  });
  let previousFrameId = '';
  $effect(() => {
    if (frame && frame.id !== previousFrameId) {
      previousFrameId = frame.id;
      original = false;
      holdingOriginal = false;
      cleanReady = '';
      imageError = false;
      selectedId = '';
      notice = null;
      lastNotice = '';
      clearTimeout(noticeTimer);
      presented = '';
    }
  });
  let selected = $derived(session?.blocks.find(block => block.id === selectedId));
  let busy = $derived(['recognizing', 'translating'].includes(session?.job?.stage ?? 'idle'));
  let progress = $derived(session?.job?.stage === 'recognizing' ? t("识别中") : t("翻译中"));
  let scale = $derived(
    floating
      ? (frame ? Math.max(0, Math.min((width - imagePadding * 2) / frame.width, (height - controlsHeight - imagePadding * 2) / frame.height)) : 1)
      : frame
        ? Math.min((width / (comparing ? 2 : 1) - (comparing ? 8 : 0)) / frame.width, height / frame.height)
        : 1
  );

  let scaleX = $derived(floating && frame ? imageWidth / frame.width : scale);
  let scaleY = $derived(floating && frame ? imageHeight / frame.height : scale);
  let contentWidth = $derived(frame ? frame.width * scaleX * (!floating && comparing ? 2 : 1) + (!floating && comparing ? 16 : 0) : 0);
  let contentHeight = $derived(frame ? frame.height * scaleY : 0);
  const handles = $derived([
    ['nw',t("左上角"),'NorthWest'], ['n',t("上边"),'North'], ['ne',t("右上角"),'NorthEast'], ['e',t("右边"),'East'],
    ['se',t("右下角"),'SouthEast'], ['s',t("下边"),'South'], ['sw',t("左下角"),'SouthWest'], ['w',t("左边"),'West'],
  ] as const);
  let resizing: { id:number;node:HTMLElement;start:Promise<unknown>;direction:string;anchor:{left:number;top:number;right:number;bottom:number;width:number;height:number} } | null = null;
  let resizePreview = $state<{left:number;top:number;width:number;height:number}|null>(null);
  let previewFrame=0;
  let resizeEnding=false;
  let nativeOutlineReady=$state(false);
  function paintResizePreview() {
    const gesture=resizing;if(!gesture)return;
    const anchor=gesture.anchor,dpr=devicePixelRatio || 1;
    const width=Math.round((resultLayout?.imageWidth ?? anchor.width)*dpr)/dpr;
    const height=Math.round((resultLayout?.imageHeight ?? anchor.height)*dpr)/dpr;
    const west=gesture.direction.includes('West'),north=gesture.direction.includes('North');
    resizePreview={left:Math.round(((west?anchor.right-width:anchor.left)-screenX)*dpr)/dpr,top:Math.round(((north?anchor.bottom-height:anchor.top)-screenY)*dpr)/dpr,width,height};
    // Geometry is committed synchronously in this RAF, before other observers or
    // the compositor read the native origin. Waiting for Svelte's next flush
    // leaves one old edge at the new window position during west/north resizing.
    const surface=gesture.node.closest('.floating-canvas-stage') as HTMLElement;
    Object.assign(surface.style,{left:resizePreview.left+'px',right:'auto',top:resizePreview.top+'px',width:width+'px',height:height+'px'});
    previewFrame=requestAnimationFrame(paintResizePreview);
  }
  let resizeFrame = 0;
  let resizeWrite: Promise<unknown> | null = null;
  let resizeDirty = false;
  function resize(event: PointerEvent,direction: typeof handles[number][2]) {
    event.preventDefault();event.stopPropagation();
    if (!desktop || event.button !== 0 || resizing || resizeEnding) return;
    const node=event.currentTarget as HTMLElement;
    try { node.setPointerCapture(event.pointerId); } catch { /* Test pointers do not have native capture. */ }
    const rect=node.closest('.floating-canvas-stage')!.getBoundingClientRect();
    resizing={id:event.pointerId,node,start:call('begin_result_resize',{direction}),direction,anchor:{left:screenX+rect.left,top:screenY+rect.top,right:screenX+rect.right,bottom:screenY+rect.bottom,width:rect.width,height:rect.height}};
    paintResizePreview();
    const gesture=resizing;
    resizing.start.then(()=>{if(resizing===gesture)nativeOutlineReady=true;}).catch(() => { void endResize(); });
  }
  function resizeMove(event: PointerEvent) {
    if (!resizing || resizeEnding || resizing.id !== event.pointerId) return;
    event.preventDefault(); resizeDirty=true;
    if (!resizeFrame) resizeFrame=requestAnimationFrame(() => { resizeFrame=0; void writeResize(); });
  }
  async function writeResize() {
    const gesture=resizing;
    if (!gesture || !resizeDirty || resizeWrite) return;
    resizeDirty=false;
    const write=gesture.start.then(() => call('update_result_resize',{finish:false}));
    resizeWrite=write;
    try { await write; } catch { void endResize(); }
    finally { if (resizeWrite === write) resizeWrite=null; if (resizeDirty && !resizeEnding && resizing === gesture && !resizeFrame) resizeFrame=requestAnimationFrame(() => { resizeFrame=0;void writeResize(); }); }
  }
  async function endResize(event?: PointerEvent) {
    const gesture=resizing;
    if (!gesture || resizeEnding || (event && event.pointerId !== gesture.id)) return;
    if (resizeDirty && !resizeWrite) void writeResize();
    const flushLatest = resizeDirty;
    // Keep the preview tracking native layout until the final pending write settles.
    // A second gesture must not start while the first one restores its outline.
    resizeEnding=true; resizeDirty=false;
    if (resizeFrame) { cancelAnimationFrame(resizeFrame);resizeFrame=0; }
    if (gesture.node.hasPointerCapture?.(gesture.id)) gesture.node.releasePointerCapture(gesture.id);
    try {
      await gesture.start;
      await resizeWrite;
      if (flushLatest) await call('update_result_resize',{finish:false});
    } catch { /* Restore tools even after a failed resize request. */ }
    finally {
      // Keep the fixed screen anchor until the last native geometry and IPC layout
      // have both painted. Returning to viewport CSS before then caused a release jump.
      await tick();await new Promise(resolve=>{const fallback=setTimeout(resolve,80);requestAnimationFrame(()=>requestAnimationFrame(()=>{clearTimeout(fallback);resolve(undefined);}));});
      try {
        const finalLayout=await call<ResultLayout>('result_layout');if(finalLayout)resultLayout=finalLayout;
        const size=await getCurrentWindow().innerSize();
        for(let i=0;i<12 && !disposed && !document.hidden && (Math.abs(innerWidth*devicePixelRatio-size.width)>.5 || Math.abs(innerHeight*devicePixelRatio-size.height)>.5);i++)await Promise.race([new Promise(requestAnimationFrame),new Promise(resolve=>setTimeout(resolve,60))]);
        await tick();
      } catch { /* Restore even when a native window was closed during release. */ }
      if(previewFrame){cancelAnimationFrame(previewFrame);previewFrame=0;}
      resizing=null;resizePreview=null;nativeOutlineReady=false;
      // Paint the final CSS outline before retiring its native counterpart.
      await tick();
      await new Promise(resolve=>{const fallback=setTimeout(resolve,80);requestAnimationFrame(()=>requestAnimationFrame(()=>{clearTimeout(fallback);resolve(undefined);}));});
      try { await call('update_result_resize',{finish:true}); } catch { /* Window may already be closed. */ }
      finally { resizeEnding=false; }
    }
  }
  onDestroy(() => { void endResize(); });

  $effect(() => {
    const key = selected ? `${selected.id}:${selected.revision}` : '';
    if (key !== previousKey) {
      previousKey = key;
      draftText = selected?.text ?? '';
      draftTarget = selected?.targetLanguage || 'auto';
    }
  });

  function edit(id: string) { selectedId = id; editorTab='source'; }
  let edited = $derived(!!selected && (draftText !== selected.text || draftTarget !== (selected.targetLanguage || 'auto')));
  let blockBusy = $derived(!!selected && submittedEdit?.id === selected.id && selected.text === submittedEdit.text && selected.targetLanguage === submittedEdit.target && ['recognizing','translating'].includes(session?.job.stage ?? ''));
  async function retranslateBlock() {
    if(!selected || !draftText.trim() || blockBusy) return;
    submittedEdit={id:selected.id,text:draftText,target:draftTarget};
    await act(call('edit_block',{id:selected.id,text:draftText,target:draftTarget}));
  }


  function showNotice(text: string, tone: 'normal' | 'error' = 'normal') {
    if (toolsOnly && desktop) { void call('show_result_notice',{text,tone}).catch(()=>{});return; }
    clearTimeout(noticeTimer);
    notice = { text, tone };
    if (desktop && floating && contentHeight < 60) void call('quiet_result_toolbar',{quiet:true}).catch(()=>{});
    noticeTimer = setTimeout(() => { notice=null; if(desktop&&floating)void call('quiet_result_toolbar',{quiet:false}).catch(()=>{}); }, 2600);
  }
  $effect(() => {
    if (!floating || toolsOnly) return;
    const error = session?.job.error;
    const key = imageError ? 'image:' + frame?.id : error ? session?.job.id + ':' + error.message : session?.job.stage === 'cancelled' ? 'cancelled:' + session.job.id : '';
    if (key && key !== lastNotice) {
      lastNotice = key;
      showNotice(imageError ? t("截图读取失败") : error ? screenshotNotice(error) : t("已取消"), error || imageError ? 'error' : 'normal');
    }
  });
  onDestroy(() => clearTimeout(noticeTimer));
  async function handleCopy(text: string, extracted = false) {
    if (!text) { if (extracted) showNotice(t("没有可提取的文字")); return; }
    if (floating) {
      try { await call('copy_text', { text }); }
      catch { showNotice(t("复制失败，请重试"), 'error'); return; }
    } else if (!await copy(text)) return;
    const kind = extracted ? 'source' : 'translation';
    clearTimeout(copyTimers[kind]);
    copied[kind] = true;
    copyTimers[kind] = setTimeout(() => copied[kind] = false, 1400);
  }
  async function dockToolbar(above: boolean, alignLeft: boolean) {
    const frameId = frame?.id;
    dockWrite = dockWrite.then(async () => {
      if (frame?.id !== frameId) return;
      if (desktop) {
        const layout = await call<ResultLayout>('set_result_toolbar', { above, alignLeft });
        if (layout?.frameId === frame?.id) { if(toolsOnly){controlsAbove=layout.controlsAbove;toolbarAlignLeft=layout.toolbarAlignLeft??false;}else resultLayout=layout; }
      } else { controlsAbove = above; toolbarAlignLeft = alignLeft; }
    }).catch(() => showNotice(t("无法调整工具栏"), 'error'));
    await dockWrite;
  }
  function beginToolbar(event: PointerEvent) {
    if (desktop && toolsOnly && event.button === 0) {
      event.preventDefault();event.stopPropagation();
      void call('begin_toolbar_drag').then(()=>getCurrentWindow().startDragging()).then(()=>call('finish_toolbar_drag')).catch(()=>showNotice(t("无法移动工具栏"),'error'));
      return;
    }
    if (event.button !== 0 || resizing) return;
    event.preventDefault(); event.stopPropagation();
    const node = event.currentTarget as HTMLElement;
    try { node.setPointerCapture(event.pointerId); } catch { /* Synthetic pointer. */ }
    toolbarGesture = { id: event.pointerId, node, x: event.clientX, y: event.clientY };
  }
  function moveToolbar(event: PointerEvent) {
    if (toolbarGesture?.id !== event.pointerId) return;
    const x = event.clientX - toolbarGesture.x, y = event.clientY - toolbarGesture.y;
    if (Math.hypot(x, y) >= 5) toolbarDragging = true;
    if (toolbarDragging) toolbarOffset = { x, y };
  }
  function endToolbar(event: PointerEvent) {
    const gesture = toolbarGesture;
    if (!gesture || gesture.id !== event.pointerId) return;
    toolbarGesture = null;
    if (gesture.node.hasPointerCapture?.(event.pointerId)) gesture.node.releasePointerCapture(event.pointerId);
    const dragged = toolbarDragging;
    toolbarDragging = false; toolbarOffset = { x: 0, y: 0 };
    if (dragged && event.type === 'pointerup') void dockToolbar(event.clientY < (controlsAbove ? 56 : 8) + contentHeight / 2, event.clientX < width / 2);
  }
  function toolbarKey(event: KeyboardEvent) {
    if (!['ArrowUp','ArrowDown','ArrowLeft','ArrowRight'].includes(event.key)) return;
    event.preventDefault(); event.stopPropagation();
    void dockToolbar(event.key === 'ArrowUp' ? true : event.key === 'ArrowDown' ? false : controlsAbove, event.key === 'ArrowLeft' ? true : event.key === 'ArrowRight' ? false : toolbarAlignLeft);
  }
  function setDisplay(nextOriginal:boolean, nextComparing:boolean, holding=false) {
    original=nextOriginal;comparing=nextComparing;holdingOriginal=holding;
    if(floating&&desktop)void call('set_result_display',{original,comparing,holdingOriginal:holding}).catch(()=>{});
  }
  function retryOrRecover() {
    if (session?.job.error?.action) recover(session.job.error.action);
    else void act(call('retry_visual'));
  }

  function dragWindow(event: PointerEvent) {
    if (!floating || !desktop || event.button !== 0 || imageDragging || resizing) return;
    const target = event.target;
    if (target instanceof Element && target.closest('button:not(.hud-grip), input, textarea')) return;
    event.preventDefault();
    imageDragging = true;
    void (async () => {
      try {
        await call('begin_result_drag');
        await getCurrentWindow().startDragging();
      } catch { showNotice(t("无法移动截图，请重试"), 'error'); }
      finally {
        await call('finish_result_drag').catch(()=>{});
        imageDragging = false;
      }
    })();
  }

  function keydown(event: KeyboardEvent) {
    if (event.isComposing || event.ctrlKey || event.metaKey || event.altKey) return;
    if (event.target instanceof Element && event.target.closest('button, input, textarea, select, [contenteditable="true"], [role="button"], [role="combobox"], [role="listbox"], [role="menu"], [role="menuitem"], [role="dialog"]')) return;
    if (selectedId) return;
    if (event.key.toLowerCase() === 'r' && !busy) { event.preventDefault(); void act(call('retry_visual')); return; }
    if (event.key.toLowerCase() === 'e' && session?.blocks[0] && !floating) { event.preventDefault(); edit(session.blocks[0].id); return; }
    if (event.key === ' ') {
      event.preventDefault();
      setDisplay(original, comparing, true);
    }
    if (floating) {
      if (event.key.toLowerCase() === 'c' && !event.ctrlKey && !event.metaKey && session?.output) {
        event.preventDefault();
        handleCopy(session.output);
      }
      if (event.key === 'Escape' || event.key.toLowerCase() === 'm') {
        void act(call('hide_window'));
      }
      const arrows: Record<string, [number, number]> = {
        ArrowLeft: [-1, 0],
        ArrowRight: [1, 0],
        ArrowUp: [0, -1],
        ArrowDown: [0, 1]
      };
      if (arrows[event.key]) {
        event.preventDefault();
        const step = event.shiftKey ? 10 : 1;
        void act(call('nudge_result', { dx: arrows[event.key][0] * step, dy: arrows[event.key][1] * step }));
      }
    }
  }
</script>

<svelte:window
  onkeydown={keydown}
  onkeyup={event => {
    if (event.key === ' ' && holdingOriginal) setDisplay(original, comparing, false);
  }}
  onblur={() => { if(holdingOriginal)setDisplay(original,comparing,false); }}
/>

<div class="result-workspace" class:floating bind:clientWidth={workspaceWidth}>
  {#if toolsOnly}<div class="toolbar-window">{@render floatingToolbar()}</div>
  {:else if !frame}
    <div class="capture-empty">
      <span class="capture-empty-symbol" aria-hidden="true"><ScanLine size={28}/></span>
      <div class="empty-actions">
        <Button onclick={() => act(call('begin_capture', { monitor: null }))}>
          {t("开始截图")}
        </Button>
        <Button variant="outline" onclick={() => act(call('open_image'))}>
          {t("打开图片")}
        </Button>
      </div>
    </div>
  {:else if floating}
    <div class="floating-shell" class:controls-above={controlsAbove} class:image-align-left={imageAlignLeft} bind:clientWidth={width} bind:clientHeight={height}>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="floating-canvas-stage"
      class:dragging={imageDragging}
      class:native-resizing={nativeOutlineReady && /Windows/.test(navigator.userAgent)}
      style:left={resizePreview ? resizePreview.left+'px' : undefined}
      style:right={resizePreview ? 'auto' : undefined}
      style:top={resizePreview ? resizePreview.top+'px' : undefined}
      style:width={resizePreview ? resizePreview.width+'px' : undefined}
      style:height={resizePreview ? resizePreview.height+'px' : undefined}
      style:--image-width={`${resultLayout?.imageWidth ?? Math.max(0,frame.width * scale)}px`}
      bind:clientWidth={imageWidth}
      bind:clientHeight={imageHeight}
      onpointerdown={dragWindow}
      oncontextmenu={e => {
        e.preventDefault();
      }}
    >
      <div class="image-viewport"><div class="floating-image-plane">
      <img
        class="floating-base-img"
        data-frame-id={frame.id}
        src={frameUrl(frame.id)}
        alt={t("截图原文")}
        draggable="false"
        onload={originalLoaded}
        onerror={() => { imageError = true; void revealReady(); }}
      />
      <div class="floating-translation-layer" style:visibility={showOriginal || cleanReady !== frame.cleanId ? 'hidden' : 'visible'} style:clip-path={comparing && !showOriginal ? `inset(0 0 0 ${comparePosition}%)` : undefined}>
        {#if frame.cleanId}<img class="floating-base-img" data-frame-id={frame.cleanId} src={frameUrl(frame.cleanId)} alt={t("清理后的截图")} draggable="false" onload={cleanLoaded}/>{/if}
        <div class="floating-blocks-layer" style:width={`${frame.width}px`} style:height={`${frame.height}px`} style:transform={`scale(${scaleX}, ${scaleY})`}>
          {#each session?.blocks.filter(block => block.translation) ?? [] as block (block.id)}
            {@const fitted = fitOverlayText(block.translation, block.bounds.width, block.bounds.height, sourceFontSize(block))}
            <div
              class="floating-block"
              class:untranslated={!block.translation}
              style:left={`${block.bounds.x}px`}
              style:top={`${block.bounds.y}px`}
              style:width={`${block.bounds.width}px`}
              style:height={`${block.bounds.height}px`}
              style:font-size={`${fitted.size}px`}
              style:line-height={fitted.lineHeight}
              style:color={block.foregroundColor || 'inherit'}
            >
              <span class="block-content">{block.translation || block.text}</span>
            </div>
          {/each}
        </div>
      </div>
      </div>
      {#if comparing && !showOriginal}
        <div class="comparison-divider" style:left={`${comparePosition}%`}><span><ArrowRightLeft size={16}/></span></div>
        <input class="comparison-slider" type="range" min="0" max="100" step="0.1" bind:value={comparePosition} aria-label={t("原译分界")}/>
      {/if}
      </div>
      <div class="result-selection-border">{#each handles as [position,label,direction]}<button class={`resize-handle ${position}`} aria-label={t("调整{0}大小",label)} onpointerdown={event => resize(event,direction)} onpointermove={resizeMove} onpointerup={endResize} onpointercancel={endResize} onlostpointercapture={endResize}></button>{/each}</div>

    </div>
    <div class="floating-controls" class:toolbar-left={toolbarAlignLeft} class:notice-compact={!!notice && contentHeight < 60} inert={!!notice && contentHeight < 60} style:top={controlsAbove ? `${imagePadding}px` : `${frame.height * scaleY + imagePadding}px`}>
      {#if !desktop}{@render floatingToolbar()}{/if}

    </div>
      {#if notice}<div class="overlay-notice" role={notice.tone === 'error' ? 'alert' : 'status'} style:top={contentHeight < 60 ? '50%' : ((controlsAbove ? 56 : 8) + contentHeight / 2) + 'px'} transition:fade={{ duration: prefersReducedMotion.current ? 0 : 120 }}>{notice.text}</div>{/if}
    </div>
  {:else}
    <!-- Non-Floating Main Window View -->
    <div class="result-toolbar">
      <div class="result-tools">
        <button class="icon-button" title={t("打开图片")} aria-label={t("打开图片")} onclick={() => act(call('open_image'))}><FolderOpen size={20}/></button>
        <button
          class="icon-button"
          class:active={showOriginal}
          title={t("显示原文（按住空格）")}
          aria-label={t("显示原文")}
          onclick={() => (original = !original)}
        >
          <Eye size={20} />
        </button>
        <button
          class="icon-button"
          title={t("复制全部译文")}
          disabled={!session?.output}
          onclick={() => session?.output && handleCopy(session.output)}
        >
          <Copy size={20} />
        </button>
        <button
          class="icon-button"
          title={t("重新翻译 (R)")}
          disabled={busy}
          onclick={() => act(call('retry_visual'))}
        >
          <RotateCcw size={20} class={busy ? 'spin' : ''} />
        </button>
        <button
          class="icon-button"
          title={t("校对文字 (E)")}
          disabled={!(session?.blocks?.length)}
          onclick={() => session?.blocks?.[0] && edit(session.blocks[0].id)}
        >
          <PencilLine size={20} />
        </button>
        <button class="icon-button" aria-label={t("提取文字")} title={t("提取并复制识别的原文")} disabled={!sourceText} onclick={() => handleCopy(sourceText, true)}><TextScan size={20}/></button>
        <Button
          variant="outline"
          size="sm"
          onclick={() => act(call('begin_capture', { monitor: null }))}
        >
          <ScanLine size={14} />{t("重新截图")}
        </Button>
      </div>
    </div>

    <div class="result-body">
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="image-stage"
        bind:clientWidth={width}
        bind:clientHeight={height}
        oncontextmenu={e => {
          e.preventDefault();
          original = !original;
        }}
        role="region"
        aria-label={t("截图翻译预览")}
      >
        <div class="result-image-plane" style:width={`${contentWidth}px`} style:height={`${contentHeight}px`} style:transform="translate(-50%, -50%)">
        {#if comparing}
          <div
            class="image-canvas"
            style:width={`${frame.width * scale}px`}
            style:height={`${frame.height * scale}px`}
          >
            <img src={frameUrl(frame.id)} alt={t("截图原文")} draggable="false" />
            <span class="canvas-label">{t("原文")}</span>
          </div>
        {/if}

        <div
          class="image-canvas"
          style:width={`${frame.width * scale}px`}
          style:height={`${frame.height * scale}px`}
        >
          <img
            data-frame-id={frame.cleanId || frame.id}
            src={frameUrl(showOriginal ? frame.id : (frame.cleanId || frame.id))}
            alt={t("截图与翻译结果")}
            draggable="false"
            crossorigin="anonymous"
            onload={cleanLoaded}
          />
          {#if !showOriginal && frame.cleanId && cleanReady === frame.cleanId}
            {#each session?.blocks ?? [] as block (block.id)}
              {@const fitted = fitOverlayText(block.translation || block.text, block.bounds.width * scale, block.bounds.height * scale, sourceFontSize(block) * scale)}
              <button
                class="translated-block"
                class:selected={block.id === selectedId}
                class:untranslated={!block.translation}
                style:left={`${block.bounds.x * scale}px`}
                style:top={`${block.bounds.y * scale}px`}
                style:width={`${block.bounds.width * scale}px`}
                style:height={`${block.bounds.height * scale}px`}
                style:font-size={`${fitted.size}px`}
                style:line-height={fitted.lineHeight}
                style:color={block.foregroundColor || 'inherit'}
                title={block.translation || block.text}
                onclick={() => edit(block.id)}
              >
                {block.translation || block.text}
              </button>
            {/each}
          {/if}
        </div>

        </div>
        {#if busy}
          <div class="image-processing">
            <LoaderCircle size={20} class="spin" />
            <span>{progress}</span>
            <Button variant="ghost" size="sm" onclick={() => act(call('cancel_job', { session: 'visual' }))}>{t("取消")}</Button>
          </div>
        {/if}

        {#if session?.job?.error}
          <div class="image-error">
            <p>{session.job.error.message}</p>
            <Button
              size="sm"
              variant="outline"
              onclick={() =>
                session?.job?.error?.action ? recover(session.job.error.action) : act(call('retry_visual'))}
            >
              {t("处理问题")}<ArrowUpRight size={13} />
            </Button>
          </div>
        {/if}
      </div>

    </div>

  {/if}
</div>

<Dialog.Root open={!!selected && !floating} onOpenChange={open => { if (!open) selectedId = ''; }}>
  <Dialog.Portal><Dialog.Overlay class="dialog-overlay"/><Dialog.Content class={compactEditor ? "editor-dialog editor-tabbed" : "editor-dialog"}>
    <Dialog.Description class="sr-only">{t("左右对照校对原文与译文，只重译当前文字块。")}</Dialog.Description>
    <div class="block-editor-heading"><Dialog.Title>{t("校对文字")}</Dialog.Title><button class="icon-button" aria-label={t("关闭校对")} onclick={() => selectedId = ''}><X size={20}/></button></div>
    <div class="block-editor-actions">
      <Choice id="block-language" class="field" label={t("目标语言")} value={draftTarget} options={languageOptions().map(([code,name]) => [code,code === 'auto' ? t("智能互译") : name])} onchange={value => draftTarget=value}/>
      <Button size="sm" disabled={!draftText.trim() || blockBusy} onclick={retranslateBlock}>{#if blockBusy}<LoaderCircle size={16} class="spin"/>{:else}<RotateCcw size={16}/>{/if}{t("重译此块")}</Button>
    </div>
    {#if compactEditor}
      <Tabs.Root bind:value={editorTab} class="correction-tabs"><Tabs.List class="editor-pane-tabs" aria-label={t("校对内容")}><Tabs.Trigger value="source">{t("原文")}</Tabs.Trigger><Tabs.Trigger value="target">{t("译文")}</Tabs.Trigger></Tabs.List><Tabs.Content value="source">{@render correctionSource()}</Tabs.Content><Tabs.Content value="target">{@render correctionTarget()}</Tabs.Content></Tabs.Root>
    {:else}<div class="block-editor-fields">{@render correctionSource()}{@render correctionTarget()}</div>{/if}
    {#if edited || blockBusy}<div class="editor-pending" role="status">{blockBusy ? t("正在更新当前文字块…") : t("原文或语言已修改，译文待更新")}</div>{/if}
  </Dialog.Content></Dialog.Portal>
</Dialog.Root>

{#snippet correctionSource()}
  <section class="correction-pane"><label for="block-source">{t("识别原文")}</label><textarea id="block-source" class="field block-source" bind:value={draftText} onkeydown={event => {if((event.ctrlKey||event.metaKey) && event.key === 'Enter' && !event.isComposing){event.preventDefault();void retranslateBlock();}}}></textarea></section>
{/snippet}
{#snippet correctionTarget()}
  <section class="correction-pane"><div class="correction-pane-heading"><span>{t("当前译文")}</span>{#if selected?.translation}<CopyButton text={selected!.translation} label={t("复制当前块译文")}/>{/if}</div><div class="block-translation" role="textbox" aria-label={t("当前译文")} aria-readonly="true" tabindex="0">{selected?.translation || t("等待翻译")}</div></section>
{/snippet}

{#snippet floatingToolbar()}
      <div class="floating-hud" class:dragging={toolbarDragging} style:transform={toolbarDragging ? `translate(${toolbarOffset.x}px, ${toolbarOffset.y}px)` : undefined} role="toolbar" aria-label={t("截图操作")}>
        <button class="hud-grip" aria-label={t("移动工具栏")} title={t("拖动自由放置，双击靠回图片")} onpointerdown={beginToolbar} onpointermove={moveToolbar} onpointerup={endToolbar} onpointercancel={endToolbar} onlostpointercapture={endToolbar} ondblclick={() => dockToolbar(!controlsAbove, toolbarAlignLeft)} onkeydown={toolbarKey}><BrandHandle size={20}/></button>
        <div class="hud-sep"></div>
        {#if busy}
          <span class="hud-progress" role="status"><LoaderCircle size={14} class="spin"/>{progress}</span>
          <button class="hud-item" aria-label={t("取消截图翻译")} onclick={() => act(call('cancel_job', { session: 'visual' }))}>{t("取消")}</button>
        {:else}
        <button
          class="hud-item"
          class:active={showOriginal}
          aria-label={t("切换原文")}
          aria-pressed={showOriginal}
          title={t("按住空格查看原文")}
          onclick={() => setDisplay(!original,comparing)}
        >
          <Eye size={20}/>
        </button>
        <button
          class="hud-item"
          aria-label={t("复制全部译文")}
          title={t("复制全部译文 (C)")}
          disabled={!session?.output}
          onclick={() => session?.output && handleCopy(session.output)}
        >
          {#if copied.translation}<Check class="copy-done" size={20}/>{:else}<Copy size={20}/>{/if}
        </button>
        <button
          class="hud-item"
          aria-label={session?.job.error?.action ? t("打开服务设置") : t("重新翻译")}
          class:has-error={!!session?.job.error}
          title={session?.job.error ? `${session.job.error.message} — ${session.job.error.action ? t("打开设置") : t("重试")}` : t("重新翻译 (R)")}
          disabled={busy}
          onclick={retryOrRecover}
        >
          {#if session?.job.error?.action}<Settings2 size={20}/>{:else}<RotateCcw size={20}/>{/if}
        </button>
        {/if}
        <button class="hud-item" aria-label={t("提取文字")} title={t("提取并复制识别的原文")} disabled={!sourceText} onclick={() => handleCopy(sourceText, true)}>{#if copied.source}<Check class="copy-done" size={20}/>{:else}<TextScan size={20}/>{/if}</button>
        <div class="hud-sep"></div>
        <button
          class="hud-item hud-close"
          aria-label={t("收起截图结果")}
          title={t("关闭 (Esc)")}
          onclick={() => act(call('hide_window'))}
        >
          <X size={20} />
        </button>
      </div>

{/snippet}
