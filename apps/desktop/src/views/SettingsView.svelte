<script lang="ts">
  import { t, uiLocale, setLocale } from '$lib/i18n.svelte';
  import ActiveIndicator from '$lib/components/ui/active-indicator.svelte';
  import { Tabs, Dialog } from 'bits-ui';
  import { onDestroy, untrack } from 'svelte';
  import { Check, Palette, LoaderCircle, Monitor, Moon, Sun, Globe, Languages, ArrowRightLeft, PanelTop, Copy, Import, RotateCcw, TextScan, Plus, Minus, ChevronDown } from '$lib/icons';
  import { toast } from 'svelte-sonner';
  import { app, saveConfig, previewConfig } from '$lib/store.svelte';
  import { call, loadBootstrap } from '$lib/api';
  import { createAutosave } from '$lib/autosave';
  import { errorMessage, languageOptions } from '$lib/utils';
  import type { Config, MigrationReport } from '$lib/generated';
  import defaults from '$lib/default-config.json';
  import { actionPalette } from '$lib/appearance';
  import Button from '$lib/components/ui/button.svelte';
  import Switch from '$lib/components/ui/switch/switch.svelte';
  import Choice from '$lib/components/ui/choice.svelte';
  import AboutView from './AboutView.svelte';
  let { onwelcome }: { onwelcome?: () => void } = $props();

  let draft = $state<Config>(structuredClone($state.snapshot(app.data!.config)));
  if (['privacy','migration','data'].includes(untrack(() => app.settingsTab))) app.settingsTab = 'general';
  let importing = $state(false); let confirmImport = $state(false);
  let lastScheduled = untrack(() => JSON.stringify(draft));
  const autosave = createAutosave<Config>(config => saveConfig(config, { quiet: true }));
  const tabs = $derived([ ['general', t("常规")], ['appearance', t("外观")], ['about', t("关于")] ] as const);
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
  $effect(() => { JSON.stringify(draft); untrack(queueDraft); });
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
      <div class="preference-card">
        <div class="setting-row"><div class="setting-row-lead"><span class="setting-row-icon" aria-hidden="true"><Globe size={16}/></span><label for="ui-language">{t("界面语言")}</label></div><Choice id="ui-language" class="field compact-field" label={t("界面语言")} value={uiLocale.current} options={[["zh-CN",t("简体中文")],["en","English"]]} onchange={setLocale}/></div>
        <div class="setting-row"><div class="setting-row-lead"><span class="setting-row-icon" aria-hidden="true"><Languages size={16}/></span><label for="auto-translate">{t("输入后自动翻译")}</label></div><Switch id="auto-translate" bind:checked={draft.autoTranslate}/></div>
        <div class="setting-row"><div class="setting-row-lead"><span class="setting-row-icon" aria-hidden="true"><ArrowRightLeft size={16}/></span><label for="chinese-target">{t("中文默认译为")}</label></div><Choice id="chinese-target" class="field compact-field" label={t("中文默认译为")} value={draft.chineseTarget} options={languageOptions().filter(([code]) => code !== 'auto' && code !== 'zh-Hans')} onchange={value => draft.chineseTarget = value}/></div>
        <div class="setting-row"><div class="setting-row-lead"><span class="setting-row-icon" aria-hidden="true"><PanelTop size={16}/></span><label for="close-tray">{t("关闭后收起到托盘")}</label></div><Switch id="close-tray" bind:checked={draft.appearance.closeToTray} disabled={!app.data?.capabilities.tray}/></div>
      </div>
      <details class="settings-more"><summary><span>{t("更多设置")}</span><ChevronDown size={17}/></summary>
        <div class="preference-card">
          <div class="setting-row"><div class="setting-row-lead"><span class="setting-row-icon" aria-hidden="true"><Monitor size={16}/></span><label for="autostart">{t("登录后自动启动")}</label></div><Switch id="autostart" bind:checked={draft.autostart}/></div>
          <div class="setting-row"><div class="setting-row-lead"><span class="setting-row-icon" aria-hidden="true"><Copy size={16}/></span><label for="auto-copy">{t("截图译文自动复制")}</label></div><Switch id="auto-copy" bind:checked={draft.appearance.autoCopy}/></div>
          {#if app.data?.legacyAvailable}<div class="setting-row"><div class="setting-row-lead"><span class="setting-row-icon" aria-hidden="true"><Import size={16}/></span><span>{t("导入旧版设置")}</span></div><Button variant="outline" size="sm" disabled={importing} onclick={() => confirmImport = true}>{#if importing}<LoaderCircle class="spin" size={15}/>{/if}{t("导入")}</Button></div>{/if}
          <div class="setting-row"><div class="setting-row-lead"><span class="setting-row-icon" aria-hidden="true"><RotateCcw size={16}/></span><span>{t("恢复默认设置")}</span></div><Button variant="outline" size="sm" title={t("保留已存密钥")} onclick={() => { draft = structuredClone(defaults) as Config; draft.onboardingComplete = true; }}>{t("恢复")}</Button></div>
        </div>
      </details>
    </Tabs.Content>
    <Tabs.Content value="appearance" class="settings-panel">
      <section class="preference-group" aria-label={t("主题")}><div class="appearance-themes">{#each themes as [value,label,Icon]}<button class="appearance-theme" class:selected={draft.appearance.theme === value} aria-pressed={draft.appearance.theme === value} onclick={() => draft.appearance.theme = value}><span class={`theme-window ${value}`} aria-hidden="true"><i></i><span><b></b><b></b></span></span><span class="theme-name"><Icon size={16}/>{label}{#if draft.appearance.theme === value}<Check size={14}/>{/if}</span></button>{/each}</div></section>
      <section class="preference-group" aria-label={t("界面")}><div class="preference-card">
        <div class="setting-row"><div class="setting-row-lead"><span class="setting-row-icon" aria-hidden="true"><TextScan size={16}/></span><span id="font-scale-label">{t("界面字号")}</span></div><div class="font-stepper" aria-labelledby="font-scale-label"><button aria-label={t("减小界面字号")} disabled={draft.appearance.fontScale <= .8} onclick={() => fontStep(-5)}><Minus size={15}/></button><output aria-label={t("界面字号")} aria-live="polite">{Math.round(draft.appearance.fontScale * 100)}%</output><button aria-label={t("增大界面字号")} disabled={draft.appearance.fontScale >= 1.5} onclick={() => fontStep(5)}><Plus size={15}/></button></div></div>
      </div></section>
      <details class="settings-more"><summary><span>{t("强调色")}</span><ChevronDown size={17}/></summary><div class="preference-card">
        <div class="setting-row"><div class="setting-row-lead"><span class="setting-row-icon" aria-hidden="true"><Palette size={16}/></span><span>{t("强调色")}</span></div><div class="accent-options">{#each accents as [name,color]}<button class="accent-swatch" style:--swatch={color === '#18181b' ? 'var(--brand)' : color} style:--swatch-foreground={actionPalette(color, draft.appearance.theme === 'dark').foreground} class:selected={draft.appearance.accent.toLowerCase() === color} aria-label={name} aria-pressed={draft.appearance.accent.toLowerCase() === color} title={name} onclick={() => draft.appearance.accent = color}>{#if draft.appearance.accent.toLowerCase() === color}<Check size={13}/>{/if}</button>{/each}<label class="accent-custom" title={t("自定义强调色")}><Plus size={15}/><input id="accent-color" aria-label={t("自定义强调色")} type="color" bind:value={draft.appearance.accent}/></label></div></div>
      </div></details>
    </Tabs.Content>
    <Tabs.Content value="about" class="settings-panel about-panel"><AboutView {onwelcome} bind:autoUpdate={draft.autoUpdate}/></Tabs.Content>
  </Tabs.Root>
</div>

<Dialog.Root bind:open={confirmImport}><Dialog.Portal><Dialog.Overlay class="dialog-overlay"/><Dialog.Content class="reading-dialog compact-dialog"><Dialog.Title>{t("导入旧版设置")}</Dialog.Title><Dialog.Description>{t("替换当前设置并导入密钥，原文件保留。加密密钥需使用原 Windows 账户。")}</Dialog.Description><div class="dialog-actions"><Dialog.Close class="button button-outline button-size-default">{t("取消")}</Dialog.Close><Button onclick={importOld}>{t("导入")}</Button></div></Dialog.Content></Dialog.Portal></Dialog.Root>
