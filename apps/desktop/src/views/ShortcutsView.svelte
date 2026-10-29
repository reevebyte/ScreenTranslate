<script lang="ts">
  import { t } from '$lib/i18n.svelte';
  import { onDestroy, untrack } from 'svelte';
  import { ScanLine, PanelTop, Languages, ArrowRight } from '$lib/icons';
  import { app, previewConfig, saveConfig, act } from '$lib/store.svelte';
  import { call } from '$lib/api';
  import { createAutosave } from '$lib/autosave';
  import type { Config } from '$lib/generated';
  let draft = $state<Config>(structuredClone($state.snapshot(app.data!.config)));
  let last = untrack(() => JSON.stringify(draft));
  const writer = createAutosave<Config>(value => saveConfig(value, { quiet: true }));
  const actions = $derived([ ['capture',t("框选翻译"),ScanLine,'begin_capture'], ['toggle',t("收起 / 显示"),PanelTop,'toggle_result'], ['quick',t("快速文字翻译"),Languages,'toggle_quick'] ] as const);
  function queue() { const json = JSON.stringify(draft); if (json === last) return; last = json; const value = structuredClone($state.snapshot(draft)); previewConfig(value); writer.schedule(value); }
  $effect(() => { JSON.stringify(draft); untrack(queue); });
  onDestroy(() => { queue(); void writer.flush(); });
  function record(event: KeyboardEvent, key: keyof Config['hotkeys']) {
    if (event.key === 'Tab') return;
    event.preventDefault();
    if (['Control','Alt','Shift','Meta'].includes(event.key)) return;
    if (['Backspace','Delete'].includes(event.key)) { draft.hotkeys[key] = ''; return; }
    if (!(event.ctrlKey || event.altKey || event.metaKey)) return;
    const modifiers = [event.ctrlKey && 'Ctrl',event.altKey && 'Alt',event.shiftKey && 'Shift',event.metaKey && 'Super'].filter(Boolean);
    modifiers.push(event.code === 'Space' ? 'Space' : event.code.startsWith('Key') ? event.code.slice(3) : event.key);
    draft.hotkeys[key] = modifiers.join('+');
  }
</script>
<div class="shortcut-workspace">
  {#if app.configError}<div class="autosave-error" role="alert"><span>{app.configError}</span><button onclick={() => { writer.schedule(structuredClone($state.snapshot(draft))); void writer.flush(); }}>{t("重试")}</button></div>{/if}
  <div class="shortcut-cluster"><div class="shortcut-actions shortcut-grid">{#each actions as [key,label,Icon,command]}
    <div class="shortcut-action" class:primary={key === 'capture'}>
      <button class="shortcut-launch" aria-label={label} onclick={() => act(call(command, command === 'begin_capture' ? { monitor: null } : {}))}><span class="shortcut-symbol"><Icon size={25}/></span><span>{label}</span></button>
      <div class="shortcut-binding"><div class="shortcut-chord" aria-hidden="true">{#if draft.hotkeys[key]}{#each draft.hotkeys[key].split('+') as part}<kbd>{part}</kbd>{/each}{:else}<span>{t("设置快捷键")}</span>{/if}</div><input class="shortcut-key" aria-label={t("{0}快捷键",label)} title={t("点击后按组合键，Backspace 清除")} readonly value={draft.hotkeys[key]} onkeydown={event => record(event,key)}/></div>
    </div>
  {/each}</div>
  {#if app.data?.capabilities.wayland}<button class="button button-outline button-size-default" onclick={() => act(call('configure_portal_shortcuts'))}>{t("配置系统快捷键")}<ArrowRight size={16}/></button>{/if}</div>
</div>
