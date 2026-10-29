<script lang="ts">
  import { t } from '$lib/i18n.svelte';
  import { onMount, tick } from 'svelte';
  import { ScanLine, X, LoaderCircle } from '$lib/icons';
  import { call } from '$lib/api';
  import { frameUrl, errorMessage } from '$lib/utils';
  import type { FrameMetadata, Rect } from '$lib/generated';
  let frame = $state<FrameMetadata | null>(null);
  let anchor = $state<{ x: number; y: number } | null>(null);
  let selection = $state<Rect | null>(null);
  let error = $state('');
  let submitting = $state(false);
  let surface: HTMLDivElement;
  let width = $state(0); let height = $state(0);
  onMount(() => { void call<FrameMetadata>('selection_frame').then(value => frame = value).catch(reason => { error = errorMessage(reason); imageReady(); }); });
  function down(event: PointerEvent) {
    if (event.button !== 0 || submitting || !frame || !width || !height) return;
    anchor = { x: event.clientX, y: event.clientY };
    selection = { x: event.clientX, y: event.clientY, width: 0, height: 0 };
    surface.setPointerCapture(event.pointerId);
  }
  function move(event: PointerEvent) {
    if (!anchor) return;
    const x = Math.max(0, Math.min(width, event.clientX)); const y = Math.max(0, Math.min(height, event.clientY));
    selection = { x: Math.min(anchor.x, x), y: Math.min(anchor.y, y), width: Math.abs(x - anchor.x), height: Math.abs(y - anchor.y) };
  }
  async function up(event: PointerEvent) {
    if (!anchor || !selection || !frame) return;
    move(event); anchor = null;
    if (surface.hasPointerCapture(event.pointerId)) surface.releasePointerCapture(event.pointerId);
    if (selection.width < 4 || selection.height < 4) { selection = null; return; }
    submitting = true;
    try { await call('select_region', { rect: { x: selection.x * frame.width / width, y: selection.y * frame.height / height, width: selection.width * frame.width / width, height: selection.height * frame.height / height } }); }
    catch (reason) { error = errorMessage(reason); submitting = false; }
  }
  function cancel() { void call('cancel_capture'); }
  function reset() { anchor = null; if (!submitting) selection = null; }
  async function imageReady() {
    await tick();
    await call('window_ready', { frameId: frame?.id ?? null }).catch(() => {});
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    void call('selection_presented').catch(() => {});
  }
</script>

<svelte:window onkeydown={event => { if (event.key === 'Escape') cancel(); }}/>
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="capture-surface" bind:this={surface} bind:clientWidth={width} bind:clientHeight={height} onpointerdown={down} onpointermove={move} onpointerup={up} onpointercancel={reset} onlostpointercapture={() => { if (anchor) reset(); }} oncontextmenu={event => { event.preventDefault(); cancel(); }}>
  {#if frame}<img class="capture-background" src={frameUrl(frame.id)} alt={t("待框选的屏幕图像")} draggable="false" onload={imageReady} onerror={() => { error = t("截图读取失败，请重新截图"); imageReady(); }}/>{/if}
  {#if selection && selection.width > 0}
    <div class="selection-rect" style:left={`${selection.x}px`} style:top={`${selection.y}px`} style:width={`${selection.width}px`} style:height={`${selection.height}px`}><span class="selection-dimensions">{Math.round(selection.width * (frame?.width ?? width) / width)} × {Math.round(selection.height * (frame?.height ?? height) / height)}</span><i class="corner tl"></i><i class="corner tr"></i><i class="corner bl"></i><i class="corner br"></i><i class="corner tm"></i><i class="corner rm"></i><i class="corner bm"></i><i class="corner lm"></i></div>
  {:else}<div class="capture-shade"></div>{/if}
  <div class="capture-guide"><ScanLine size={17}/><span>{submitting ? t("正在识别…") : t("拖动选择区域")}</span><kbd>Esc</kbd><span>{t("取消")}</span></div>
  {#if error}<div class="capture-error">{error}<button onclick={cancel}>{t("返回")}</button></div>{/if}
</div>
