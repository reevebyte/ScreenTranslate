<script lang="ts">
  import { t } from '$lib/i18n.svelte';
  import { Dialog } from 'bits-ui';
  import { ArrowRight, X, BrandHandle, Monitor, Globe } from '$lib/icons';
  import { app, saveConfig } from '$lib/store.svelte';
  import Button from './ui/button.svelte';
  let { open = $bindable(false) }: { open?: boolean } = $props();
  let comparison = $state(50);
  async function dismissed(value: boolean) {
    open = value;
    if (!value && app.data && !app.data.config.onboardingComplete) {
      const config = structuredClone($state.snapshot(app.data.config));
      config.onboardingComplete = true;
      await saveConfig(config, { quiet: true });
    }
  }
  async function start(configure: boolean) {
    const config = structuredClone($state.snapshot(app.data!.config));
    config.onboardingComplete = true;
    if (await saveConfig(config)) { open = false; app.route = configure ? 'providers' : 'shortcuts'; }
  }
</script>

<Dialog.Root {open} onOpenChange={dismissed}>
  <Dialog.Portal>
    <Dialog.Overlay class="dialog-overlay welcome-overlay"/>
    <Dialog.Content class="welcome-dialog" onOpenAutoFocus={event => { event.preventDefault(); document.querySelector<HTMLElement>('.welcome-dialog')?.focus({preventScroll:true}); }}>
      <Dialog.Close class="welcome-close icon-button" aria-label={t("稍后配置")}><X size={17}/></Dialog.Close>
      <div class="welcome-brand"><BrandHandle size={20}/><Dialog.Title>ScreenTranslate</Dialog.Title></div>
      <Dialog.Description class="sr-only">{t("拖动中间分界线，对照查看原文与译文。")}</Dialog.Description>
      <div class="welcome-comparison">
        <div class="welcome-comparison-layer" aria-hidden="true" style:clip-path={`inset(0 0 0 ${comparison}%)`}>Read the world.</div>
        <div class="welcome-comparison-layer translated" aria-hidden="true" style:clip-path={`inset(0 ${100-comparison}% 0 0)`}>读懂眼前的世界。</div>
        <span class="welcome-comparison-divider" aria-hidden="true" style:left={`${comparison}%`}></span>
        <input class="comparison-slider welcome-comparison-slider" type="range" min="0" max="100" step="0.1" bind:value={comparison} aria-label={t("原文与译文分界")} aria-valuetext={t("中文 {0}%，英文 {1}%",Math.round(comparison),Math.round(100-comparison))}/>
        <i aria-hidden="true"></i><i aria-hidden="true"></i><i aria-hidden="true"></i><i aria-hidden="true"></i>
      </div>
      <div class="welcome-actions"><Button onclick={() => start(false)}>{t("开始使用")}<ArrowRight size={16}/></Button></div>
      <div class="welcome-footer">
        <button class="text-action" onclick={() => start(true)}>{t("配置服务")}</button>
        <span class="welcome-privacy">{#if app.data?.config.ocr.engine === 'system'}<Monitor size={14}/>{t("本机识别")}{:else}<Globe size={14}/>{app.data?.config.ocr.engine === 'custom' ? t("自定义 OCR 接口") : app.data?.config.ocr.engine === 'azure' ? t("Azure 云端识别") : t("有道云端识别")}{/if}</span>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

