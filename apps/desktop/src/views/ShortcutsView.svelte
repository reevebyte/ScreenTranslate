<script lang="ts">
  import { t } from '$lib/i18n.svelte';
  import { onDestroy, untrack } from 'svelte';
  import { toast } from 'svelte-sonner';
  import { ScanLine, PanelTop, Languages, ArrowRight } from '$lib/icons';
  import { app, restoreSavedConfig, saveConfig, act, registerFlush } from '$lib/store.svelte';
  import { call } from '$lib/api';
  import { createAutosave } from '$lib/autosave';
  import { formatHotkeyParts } from '$lib/utils';
  import type { Config } from '$lib/generated';
  let isMac = $derived(app.data?.capabilities.platform === 'macos' || (typeof navigator !== 'undefined' && /Mac/.test(navigator.userAgent)));
  let draft = $state<Config>(structuredClone($state.snapshot(app.data!.config)));
  let last = untrack(() => JSON.stringify(draft));
  type HotkeyName = keyof Config['hotkeys'];
  // Hotkeys from the last successful save: the backend keeps these active when a new one is rejected.
  let saved = structuredClone($state.snapshot(draft.hotkeys));
  let lastEdited: HotkeyName | null = null;
  let destroyed = false;
  // Rows briefly outlined while their conflict notice is shown.
  let conflict = $state<Partial<Record<HotkeyName, boolean>>>({});
  const conflictTimers: Partial<Record<HotkeyName, ReturnType<typeof setTimeout>>> = {};
  function markConflict(key: HotkeyName) {
    if (destroyed) return;
    conflict[key] = true;
    clearTimeout(conflictTimers[key]);
    conflictTimers[key] = setTimeout(() => { conflict[key] = false; }, 4500);
  }
  const writer = createAutosave<Config>(async value => {
    const ok = await saveConfig(value, { quiet: true, suppressHotkeyNotice: true });
    if (ok) { saved = structuredClone(value.hotkeys); return true; }
    if (app.configErrorAction === 'hotkeys') {
      const changed = (Object.keys(saved) as HotkeyName[]).filter(name => value.hotkeys[name] !== saved[name]);
      const primary = (lastEdited && changed.includes(lastEdited)) ? lastEdited : changed[0];
      if (primary) {
        // Name the key in the shared notice, outline affected rows, and revert
        // to the last working bindings even if the view has already unmounted.
        const keys = formatHotkeyParts(value.hotkeys[primary], isMac).join(' + ');
        const rawError = app.configErrorRaw || app.configError;
        const reason = rawError.includes('已用于其他操作') ? t("{0} 已用于其他操作，已恢复原快捷键", keys)
          : rawError.includes('占用') ? t("{0} 已被其他程序占用，已恢复原快捷键", keys)
          : t("{0}；已恢复原快捷键", app.configError);
        toast.warning(reason, { id: `hotkey-${primary}` });
        changed.forEach(markConflict);
        if (destroyed) {
          const reverted: Config = { ...structuredClone(value), hotkeys: structuredClone(saved) };
          last = JSON.stringify(reverted);
          restoreSavedConfig(reverted);
        } else {
          for (const name of changed) draft.hotkeys[name] = saved[name];
        }
      }
    }
    return false;
  });
  const actions = $derived([ ['capture',t("框选翻译"),ScanLine,'begin_capture'], ['toggle',t("收起 / 显示"),PanelTop,'toggle_result'], ['quick',t("快速文字翻译"),Languages,'toggle_quick'] ] as const);
  function queue() { const json = JSON.stringify(draft); if (json === last) return; last = json; const value = structuredClone($state.snapshot(draft)); writer.schedule(value); }
  $effect(() => { JSON.stringify(draft); untrack(queue); });
  const stopFlush = registerFlush(() => { queue(); return writer.flush(); });
  onDestroy(() => { destroyed = true; stopFlush(); Object.values(conflictTimers).forEach(clearTimeout); queue(); void writer.flush(); });
  function record(event: KeyboardEvent, key: HotkeyName) {
    if (event.key === 'Tab') return;
    event.preventDefault();
    event.stopPropagation();
    if (['Control','Alt','Shift','Meta'].includes(event.key)) return;
    lastEdited = key; conflict[key] = false;
    if (['Backspace','Delete'].includes(event.key)) { draft.hotkeys[key] = ''; return; }
    if (!(event.ctrlKey || event.altKey || event.metaKey)) return;
    const modifiers = [event.ctrlKey && 'Ctrl',event.altKey && 'Alt',event.shiftKey && 'Shift',event.metaKey && 'Super'].filter(Boolean);
    modifiers.push(event.code === 'Space' ? 'Space' : event.code.startsWith('Key') ? event.code.slice(3) : event.key);
    draft.hotkeys[key] = modifiers.join('+');
  }
</script>
<div class="shortcut-workspace">
  <div class="shortcut-cluster"><div class="shortcut-actions shortcut-grid">{#each actions as [key,label,Icon,command]}
    <div class="shortcut-action" class:primary={key === 'capture'} class:conflict={!!conflict[key]}>
      <button class="shortcut-launch" aria-label={label} onclick={() => { void act(call(command, command === 'begin_capture' ? { monitor: null } : {})); }}><span class="shortcut-symbol"><Icon size={25}/></span><span>{label}</span></button>
      <div class="shortcut-binding"><div class="shortcut-chord" aria-hidden="true">{#if draft.hotkeys[key]}{#each formatHotkeyParts(draft.hotkeys[key], isMac) as part}<kbd>{part}</kbd>{/each}{:else}<span>{t("设置快捷键")}</span>{/if}</div><input class="shortcut-key" aria-label={t("{0}快捷键",label)} title={t("点击后按组合键，Backspace 清除")} readonly value={draft.hotkeys[key]} onkeydown={event => record(event,key)}/></div>
    </div>
  {/each}</div>
  {#if app.data?.capabilities.wayland && !app.data?.capabilities.globalShortcuts}<button class="button button-outline button-size-default" onclick={() => act(call('configure_portal_shortcuts'))}>{t("配置系统快捷键")}<ArrowRight size={16}/></button>{/if}</div>
</div>
