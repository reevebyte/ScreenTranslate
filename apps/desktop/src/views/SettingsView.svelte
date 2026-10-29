<script lang="ts">
  import { t, uiLocale, setLocale } from '$lib/i18n.svelte';
  import ActiveIndicator from '$lib/components/ui/active-indicator.svelte';
  import { Tabs, Dialog } from 'bits-ui';
  import { onDestroy, untrack } from 'svelte';
  import { Check, Settings2, Keyboard, ScanLine, Palette, ShieldCheck, RefreshCw, ArrowUpRight, LoaderCircle, Import, Monitor, Moon, Sun, Globe, KeyRound, RotateCcw, Languages, PanelTop, Plus, Minus, SlidersHorizontal } from '$lib/icons';
  import { toast } from 'svelte-sonner';
  import { app, saveConfig, previewConfig, refreshCapabilities, act } from '$lib/store.svelte';
  import { call, loadBootstrap } from '$lib/api';
  import { createAutosave } from '$lib/autosave';
  import { languages, errorMessage , languageOptions } from '$lib/utils';
  import type { Config, MigrationReport } from '$lib/generated';
  import defaults from '$lib/default-config.json';
  import { actionPalette } from '$lib/appearance';
  import Button from '$lib/components/ui/button.svelte';
  import Switch from '$lib/components/ui/switch/switch.svelte';
  import Choice from '$lib/components/ui/choice.svelte';
  import AboutView from './AboutView.svelte';
  import { CircleHelp } from '$lib/icons';
  let { onwelcome }: { onwelcome?: () => void } = $props();

  let draft = $state<Config>(structuredClone($state.snapshot(app.data!.config)));
  if (['privacy','migration'].includes(untrack(() => app.settingsTab))) app.settingsTab = 'data';
  let importing = $state(false); let confirmImport = $state(false);
  let editingText = $state(false);
  let lastScheduled = untrack(() => JSON.stringify(draft));
  const autosave = createAutosave<Config>(config => saveConfig(config, { quiet: true }));
  const tabs = $derived([ ['general', t("通用"), Settings2], ['appearance', t("外观"), Palette], ['data', t("数据"), ShieldCheck], ['about', t("关于"), CircleHelp] ] as const);
  const themes = $derived([['system', t("跟随系统"), Monitor], ['light', t("浅色"), Sun], ['dark', t("深色"), Moon]] as const);
  const accents = $derived([[t("默认"),'#18181b'],[t("橙色"),'#d45b37'],[t("蓝色"),'#4276b8'],[t("绿色"),'#438569'],[t("紫色"),'#8971ad']] as const);
  function fontStep(step: number) { draft.appearance.fontScale = Math.max(.8,Math.min(1.5,Math.round((draft.appearance.fontScale * 100 + step) / 5) * 5 / 100)); }
  function queueDraft() {
    const json = JSON.stringify(draft);
    if (json === lastScheduled) return;
    lastScheduled = json;
    const config = structuredClone($state.snapshot(draft));
    previewConfig(config);
    autosave.schedule(config);
  }
  $effect(() => { JSON.stringify(draft); if (!editingText) untrack(queueDraft); });
  onDestroy(() => { queueDraft(); void autosave.flush(); });
  function retrySave() { autosave.schedule(structuredClone($state.snapshot(draft))); void autosave.flush(); }
  async function importOld() {
    importing = true;
    try {
      queueDraft();
      if (!(await autosave.flush())) throw new Error(t("当前设置未能保存，请重试后再导入。"));
      const report = await call<MigrationReport>('import_legacy_config');
      app.data = await loadBootstrap();
      lastScheduled = JSON.stringify(app.data.config);
      draft = structuredClone($state.snapshot(app.data.config));
      toast.success(t("设置已导入，迁移了 {0} 项密钥",report.credentialsImported));
      report.warnings.forEach(message => toast.warning(message));
    } catch (error) { toast.error(errorMessage(error)); }
    finally { importing = false; confirmImport = false; }
  }
</script>

<div class="settings-layout settings-redesign">
  {#if app.configError}<div class="autosave-error" role="alert"><span>{t("保存失败：")}{app.configError}</span><button onclick={retrySave}>{t("重试")}</button></div>{/if}
  <Tabs.Root bind:value={app.settingsTab} class="settings-tabs">
    <Tabs.List class="settings-tab-list" aria-label={t("设置分类")}><ActiveIndicator/>{#each tabs as [value,label]}<Tabs.Trigger {value} class="settings-tab">{label}</Tabs.Trigger>{/each}</Tabs.List>
    <Tabs.Content value="general" class="settings-panel">
      <section class="preference-group"><h3><Globe size={17}/>{t("语言")}</h3><div class="preference-card"><div class="setting-row"><label for="ui-language">{t("界面语言")}</label><Choice id="ui-language" class="field compact-field" label={t("界面语言")} value={uiLocale.current} options={[["zh-CN",t("简体中文")],["en","English"]]} onchange={setLocale}/></div></div></section>
      <section class="preference-group" aria-labelledby="translation-settings"><h3 id="translation-settings"><Languages size={17}/>{t("翻译")}</h3><div class="preference-card">
        <div class="setting-row"><label for="auto-translate">{t("输入后自动翻译")}</label><Switch id="auto-translate" bind:checked={draft.autoTranslate}/></div>
        <div class="setting-row"><label for="chinese-target">{t("中文默认译为")}</label><Choice id="chinese-target" class="field compact-field" label={t("中文默认译为")} value={draft.chineseTarget} options={languageOptions().filter(([code]) => code !== 'auto' && code !== 'zh-Hans')} onchange={value => draft.chineseTarget = value}/></div>
        <div class="setting-row"><label for="auto-copy">{t("截图译文自动复制")}</label><Switch id="auto-copy" bind:checked={draft.appearance.autoCopy}/></div>
      </div></section>
      <section class="preference-group" aria-labelledby="window-settings"><h3 id="window-settings"><PanelTop size={17}/>{t("启动与窗口")}</h3><div class="preference-card">
        <div class="setting-row"><label for="autostart">{t("登录后自动启动")}</label><Switch id="autostart" bind:checked={draft.autostart}/></div>
        <div class="setting-row"><label for="close-tray">{t("关闭后收起到托盘")}</label><Switch id="close-tray" bind:checked={draft.appearance.closeToTray} disabled={!app.data?.capabilities.tray}/></div>
      </div></section>
    </Tabs.Content>
    <Tabs.Content value="appearance" class="settings-panel">
      <section class="preference-group"><h3><Palette size={17}/>{t("主题")}</h3><div class="appearance-themes">{#each themes as [value,label,Icon]}<button class="appearance-theme" class:selected={draft.appearance.theme === value} aria-pressed={draft.appearance.theme === value} onclick={() => draft.appearance.theme = value}><span class={`theme-window ${value}`} aria-hidden="true"><i></i><span><b></b><b></b></span></span><span class="theme-name"><Icon size={16}/>{label}{#if draft.appearance.theme === value}<Check size={14}/>{/if}</span></button>{/each}</div></section>
      <section class="preference-group"><h3><SlidersHorizontal size={17}/>{t("界面")}</h3><div class="preference-card">
        <div class="setting-row"><span>{t("强调色")}</span><div class="accent-options">{#each accents as [name,color]}<button class="accent-swatch" style:--swatch={color === '#18181b' ? 'var(--brand)' : color} style:--swatch-foreground={actionPalette(color, draft.appearance.theme === 'dark').foreground} class:selected={draft.appearance.accent.toLowerCase() === color} aria-label={name} aria-pressed={draft.appearance.accent.toLowerCase() === color} title={name} onclick={() => draft.appearance.accent = color}>{#if draft.appearance.accent.toLowerCase() === color}<Check size={13}/>{/if}</button>{/each}<label class="accent-custom" title={t("自定义强调色")}><Plus size={15}/><input id="accent-color" aria-label={t("自定义强调色")} type="color" bind:value={draft.appearance.accent}/></label></div></div>
        <div class="setting-row"><span id="font-scale-label">{t("界面字号")}</span><div class="font-stepper" aria-labelledby="font-scale-label"><button aria-label={t("减小界面字号")} disabled={draft.appearance.fontScale <= .8} onclick={() => fontStep(-5)}><Minus size={15}/></button><output aria-label={t("界面字号")} aria-live="polite">{Math.round(draft.appearance.fontScale * 100)}%</output><button aria-label={t("增大界面字号")} disabled={draft.appearance.fontScale >= 1.5} onclick={() => fontStep(5)}><Plus size={15}/></button></div></div>
      </div></section>
    </Tabs.Content>
    <Tabs.Content value="data" class="settings-panel">
      <section class="preference-group"><h3><Import size={17}/>{t("配置管理")}</h3><div class="preference-card">
        <div class="setting-row"><span>{t("导入旧版设置")}</span><Button variant="outline" size="sm" title={app.data?.legacyAvailable ? t("导入服务、语言与快捷键，保留原文件") : t("未检测到旧版设置")} disabled={!app.data?.legacyAvailable || importing} onclick={() => confirmImport = true}>{#if importing}<LoaderCircle class="spin" size={15}/>{/if}{t("导入")}</Button></div>
        <div class="setting-row"><span>{t("恢复默认设置")}</span><Button variant="outline" size="sm" title={t("保留已存密钥")} onclick={() => { draft = structuredClone(defaults) as Config; draft.onboardingComplete = true; }}>{t("恢复")}</Button></div>
      </div></section>
    </Tabs.Content>
    <Tabs.Content value="about" class="settings-panel about-panel"><AboutView {onwelcome} bind:autoUpdate={draft.autoUpdate}/></Tabs.Content>
  </Tabs.Root>
</div>

<Dialog.Root bind:open={confirmImport}><Dialog.Portal><Dialog.Overlay class="dialog-overlay"/><Dialog.Content class="reading-dialog compact-dialog"><Dialog.Title>{t("导入旧版设置")}</Dialog.Title><Dialog.Description>{t("替换当前设置并导入密钥，原文件保留。加密密钥需使用原 Windows 账户。")}</Dialog.Description><div class="dialog-actions"><Dialog.Close class="button button-outline button-size-default">{t("取消")}</Dialog.Close><Button onclick={importOld}>{t("导入")}</Button></div></Dialog.Content></Dialog.Portal></Dialog.Root>
