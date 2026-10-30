<script lang="ts">
  import { t } from '$lib/i18n.svelte';
  import {
    Copy,
    X,
    ArrowUpRight,
    CornerDownLeft,
    LoaderCircle,
    Check
  } from '$lib/icons';
  import { onMount, onDestroy, tick } from 'svelte';
  import { Dialog, Tabs } from 'bits-ui';
  import { listen } from '@tauri-apps/api/event';
  import { app, act, editText, translateNow, copy } from '$lib/store.svelte';
  import { call, desktop } from '$lib/api';
  import { languageOptions } from '$lib/utils';
  import ProviderIcon from '$lib/components/ProviderIcon.svelte';
  import { providerBrand } from '$lib/provider-brand';
  import { providerInfo } from '$lib/providers';
  import Choice from '$lib/components/ui/choice.svelte';
  import Button from '$lib/components/ui/button.svelte';

  let textarea: HTMLTextAreaElement;
  let composing = $state(false);
  let copiedToast = $state(false);
  let readingOpen = $state(false);
  let windowHeight = $state(320);
  let activePane = $state('source');
  let compactReading = $derived(windowHeight < 300 || ((app.data?.config.appearance.fontScale ?? 1) > 1.2 && windowHeight < 380));
  async function submit() { await translateNow(); if (compactReading) activePane = 'target'; }

  onMount(() => {
    textarea?.focus();
    textarea?.setSelectionRange(0, 0);
    void tick().then(() => { if (textarea && document.activeElement === textarea) { textarea.setSelectionRange(0, 0); textarea.scrollTop = 0; } });
    let stop: (() => void) | undefined;
    let disposed = false;
    if (desktop) {
      void listen('focus-input', () => {
        if (disposed) return;
        activePane = 'source';
        void tick().then(() => { if (!disposed) { textarea?.focus(); textarea?.select(); } });
      }).then(fn => { if (disposed) fn(); else stop = fn; }).catch(() => {});
    }
    return () => { disposed = true; stop?.(); };
  });

  let copyTimer: ReturnType<typeof setTimeout> | undefined;
  async function handleCopy(text: string) {
    if (!text || !await copy(text)) return;
    clearTimeout(copyTimer);
    copiedToast = true;
    copyTimer = setTimeout(() => { copiedToast = false; }, 1400);
  }
  onDestroy(() => clearTimeout(copyTimer));

  function handleKeydown(event: KeyboardEvent) {
    if (readingOpen || (event.target instanceof Element && event.target.closest('[role="dialog"]'))) return;
    if (event.target instanceof Element && event.target.closest('[role="option"], [role="listbox"], [role="combobox"]')) return;
    if (event.key === 'Escape') {
      void act(call('hide_window'));
      return;
    }
    const editing = event.target instanceof HTMLElement && (event.target.matches('input, textarea') || event.target.isContentEditable);
    if (!editing && (event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'c' && !window.getSelection()?.toString() && app.data?.text.output) {
      event.preventDefault();
      handleCopy(app.data.text.output);
      return;
    }
    if (event.key === 'Enter' && event.target instanceof Element && event.target.closest('button, [role="tab"]')) return;
    if (event.key === 'Enter' && !event.shiftKey && !composing && !event.isComposing) {
      event.preventDefault();
      void submit();
    }
  }
</script>

<svelte:window bind:innerHeight={windowHeight} onkeydown={handleKeydown} />

<div class="spotlight-shell" class:compact-quick={compactReading}>
  <header class="spotlight-header" data-tauri-drag-region>
    <div class="spotlight-drag-left" data-tauri-drag-region>
      <Choice class="spotlight-lang-select" label={t("快速翻译目标语言")} value={app.target} options={languageOptions().map(([code,name]) => [code,code === 'auto' ? t("智能互译") : name])} onchange={value => editText(app.input,app.source,value)}/>
      <button class="quick-provider" onclick={() => act(call('navigate_main', { route: 'providers' }))}><ProviderIcon brand={providerBrand(app.data!.config.provider,app.data!.config.providers[app.data!.config.provider]?.baseUrl)}/><span>{t(providerInfo(app.data!.config.provider).name)}</span></button>
    </div>
    <div class="spotlight-actions">{#if app.input || app.data?.text.output}<Button variant="ghost" size="sm" onclick={() => readingOpen = true}>{t("阅读")}</Button>{/if}<button class="spotlight-icon-btn" aria-label={t("打开完整窗口")} title={t("打开主窗口")} onclick={() => act(call('show_main'))}><ArrowUpRight size={18}/></button><button class="spotlight-icon-btn" aria-label={t("收起")} title={t("收起")} onclick={() => act(call('hide_window'))}><X size={18}/></button></div>
  </header>
  {#if compactReading}
    <Tabs.Root bind:value={activePane} class="quick-compact-panes">
      <Tabs.List class="compact-pane-tabs" aria-label={t("快速翻译区域")}><Tabs.Trigger value="source">{t("原文")}</Tabs.Trigger><Tabs.Trigger value="target">{t("译文")}</Tabs.Trigger></Tabs.List>
      <Tabs.Content value="source" class="quick-tab-content">{@render sourceEditor()}</Tabs.Content>
      <Tabs.Content value="target" class="quick-tab-content">{@render targetEditor()}</Tabs.Content>
    </Tabs.Root>
  {:else}
    {@render sourceEditor()}
    {@render targetEditor()}
  {/if}
  <footer class="spotlight-footer" data-tauri-drag-region>
    <div>{#if app.data?.text.output}<button class="quick-copy" aria-label={t("复制译文")} onclick={() => handleCopy(app.data!.text.output)}>{#if copiedToast}<Check size={17}/>{:else}<Copy size={17}/>{/if}{#if !copiedToast}{t("复制")}{/if}</button>{/if}</div>
    <Button disabled={!app.input.trim() || composing || app.data?.text.job.stage === 'translating'} onclick={submit}>{t("翻译")}<CornerDownLeft size={15}/></Button>
  </footer>
</div>


<Dialog.Root bind:open={readingOpen}>
  <Dialog.Portal><Dialog.Overlay class="dialog-overlay"/><Dialog.Content class="reading-dialog quick-reading">
    <div class="dialog-heading"><Dialog.Title>{t("原译对照")}</Dialog.Title><Dialog.Close class="icon-button" aria-label={t("关闭阅读")}><X size={18}/></Dialog.Close></div>
    <Dialog.Description class="sr-only">{t("完整原文与译文，可滚动阅读和选择复制。")}</Dialog.Description>
    <div class="reading-label">{t("原文")}</div><div class="reading-text">{app.input}</div>
    <div class="reading-label">{t("译文")}</div><div class="reading-text">{app.data?.text.output || t("等待翻译")}</div>
    <Button variant="outline" disabled={!app.data?.text.output} onclick={() => handleCopy(app.data!.text.output)}>{#if copiedToast}<Check size={17}/>{:else}{t("复制译文")}{/if}</Button>
  </Dialog.Content></Dialog.Portal>
</Dialog.Root>

{#snippet sourceEditor()}
  <div class="spotlight-input-wrap">
    <textarea bind:this={textarea} class="spotlight-textarea" aria-label={t("快速翻译输入")} value={app.input} placeholder={t("输入或粘贴文字…")} spellcheck="false" rows="2"
      oninput={event => editText(event.currentTarget.value,app.source,app.target,composing)}
      oncompositionstart={() => { composing = true; editText(app.input,app.source,app.target,true); }}
      oncompositionend={event => { composing = false; editText(event.currentTarget.value,app.source,app.target,false); }}></textarea>
  </div>
{/snippet}
{#snippet targetEditor()}
  <div class="spotlight-output-wrap" aria-live="polite">
    {#if app.data?.text.output}<div class="spotlight-result"><p class="spotlight-result-text">{app.data.text.output}</p></div>
    {:else if app.data?.text.job.error}<div class="spotlight-error">{app.data.text.job.error.message}</div>
    {:else if app.data?.text.job.stage === 'translating'}<div class="spotlight-loading"><LoaderCircle size={18} class="spin"/><span>{t("正在翻译")}</span></div>
    {:else}<span class="quick-output-placeholder">{t("译文")}</span>{/if}
  </div>
{/snippet}
