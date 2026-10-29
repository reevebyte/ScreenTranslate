<script lang="ts">
  import CopyButton from '$lib/components/CopyButton.svelte';
  import { t } from '$lib/i18n.svelte';
  import * as Resizable from '$lib/components/ui/resizable/index';
  import { Tabs } from 'bits-ui';
  import { LoaderCircle, ArrowRightLeft } from '$lib/icons';
  import { app, editText, translateNow, recover, act } from '$lib/store.svelte';
  import { call } from '$lib/api';
  import { languageOptions } from '$lib/utils';
  import Button from '$lib/components/ui/button.svelte';
  import Choice from '$lib/components/ui/choice.svelte';
  import { onMount } from 'svelte';
  import Skeleton from '$lib/components/ui/skeleton/skeleton.svelte';
  import { fade } from 'svelte/transition';
  import { prefersReducedMotion } from 'svelte/motion';
  let width = $state(1000);
  let height = $state(500);
  let composing = $state(false);
  let activePane = $state('source');
  let compactReading = $derived(width < 470 && height < 310 * (app.data?.config.appearance.fontScale ?? 1));
  let textarea: HTMLTextAreaElement;
  let busy = $derived(app.data?.text.job.stage === 'translating');
  let error = $derived(app.data?.text.job.error);
  function swap() { if (app.data?.text.output) editText(app.data.text.output, app.target, app.source); }
  async function submit() { await translateNow(); if (compactReading) activePane = 'target'; }
  onMount(() => textarea?.focus());
</script>

<div class="text-workspace" bind:clientWidth={width} bind:clientHeight={height}>
  <div class="workspace-toolbar">
    <div class="language-controls">
      <Choice label={t("源语言")} value={app.source} options={languageOptions()} onchange={value => editText(app.input, value, app.target)}/>
      <button class="text-action language-swap" aria-label={t("交换原文与译文")} disabled={!app.data?.text.output} title={t("交换原文与译文")} onclick={swap}><ArrowRightLeft size={18}/></button>
      <Choice label={t("目标语言")} value={app.target} options={languageOptions().map(([key, name]) => [key, key === 'auto' ? t("智能互译") : name])} onchange={value => editText(app.input, app.source, value)}/>
    </div>
    <div class="translation-submit">{#if busy}<Button variant="outline" aria-label={t("取消翻译")} onclick={() => act(call('cancel_job', { session: 'text' }))}><LoaderCircle size={17} class="spin"/>{t("取消")}</Button>{:else}<Button disabled={!app.input.trim() || composing} onclick={submit}>{t("翻译")}</Button>{/if}</div>
  </div>
  <div class="translation-card">
    {#if compactReading}
      <Tabs.Root bind:value={activePane} class="compact-translation">
        <Tabs.List class="compact-pane-tabs" aria-label={t("翻译区域")}><Tabs.Trigger value="source">{t("原文")}</Tabs.Trigger><Tabs.Trigger value="target">{t("译文")}</Tabs.Trigger></Tabs.List>
        <Tabs.Content value="source" class="translation-pane source-pane">{@render sourceEditor()}</Tabs.Content>
        <Tabs.Content value="target" class="translation-pane target-pane">{@render targetEditor()}</Tabs.Content>
      </Tabs.Root>
    {:else}
      <Resizable.PaneGroup direction={width < 470 ? 'vertical' : 'horizontal'} class="translation-panes">
        <Resizable.Pane defaultSize={50} minSize={30} class="translation-pane source-pane">{@render sourceEditor()}</Resizable.Pane>
        <Resizable.Handle class="pane-resizer" aria-label={t("调整原文与译文的宽度")}/>
        <Resizable.Pane defaultSize={50} minSize={30} class="translation-pane target-pane">{@render targetEditor()}</Resizable.Pane>
      </Resizable.PaneGroup>
    {/if}
  </div>
</div>

{#snippet sourceEditor()}
  <div class="pane-heading"><span>{t("原文")}</span>{#if app.input}<button class="text-action" aria-label={t("清空原文")} onclick={() => editText('')}>{t("清空")}</button>{/if}</div>
  <textarea class="translation-input" bind:this={textarea} aria-label={t("待翻译的文字")} value={app.input} placeholder={t("输入或粘贴文字…")} spellcheck="false"
    oninput={event => editText(event.currentTarget.value, app.source, app.target, composing)}
    oncompositionstart={() => { composing = true; editText(app.input, app.source, app.target, true); }}
    oncompositionend={event => { composing = false; editText(event.currentTarget.value, app.source, app.target, false); }}
    onkeydown={event => { if ((event.ctrlKey || event.metaKey) && event.key === 'Enter' && !composing && !event.isComposing) { event.preventDefault(); void submit(); } }}></textarea>
{/snippet}
{#snippet targetEditor()}
  <div class="pane-heading"><span>{t("译文")}</span>{#if app.data?.text.output}<CopyButton text={app.data!.text.output} label={t("复制译文")}/>{/if}</div>
  {#if app.data?.text.output}
    <div class="translation-output" in:fade={{ duration: prefersReducedMotion.current ? 0 : 140 }} role="textbox" aria-label={t("翻译结果")} aria-readonly="true" aria-multiline="true" tabindex="0">{app.data.text.output}</div>
  {:else if busy}
    <div class="translation-skeleton" role="status" aria-label={t("正在翻译")}><Skeleton class="skeleton-line wide"/><Skeleton class="skeleton-line"/><Skeleton class="skeleton-line short"/></div>
  {:else if error}
    <div class="translation-empty error-empty"><p>{error.message}</p><Button variant="outline" onclick={() => error?.action ? recover(error.action) : submit()}>{error.action ? t("打开设置") : t("重试")}</Button></div>
  {:else}<div class="translation-empty" aria-label={t("等待翻译")}></div>{/if}
{/snippet}
