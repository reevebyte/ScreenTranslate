<script lang="ts">
  import { t } from '$lib/i18n.svelte';
  import { onDestroy, untrack } from 'svelte';
  import { Globe, Monitor, RefreshCw, ArrowUpRight, ChevronDown } from '$lib/icons';
  import { app, previewConfig, saveConfig, refreshCapabilities, act } from '$lib/store.svelte';
  import { call } from '$lib/api';
  import { createAutosave } from '$lib/autosave';
  import type { Config } from '$lib/generated';
  import ProviderIcon from '$lib/components/ProviderIcon.svelte';
  import { modelBrand, providerBrand } from '$lib/provider-brand';
  import type { ProviderBrand } from '$lib/provider-brand';
  import Switch from '$lib/components/ui/switch/switch.svelte';
  import SecretField from '$lib/components/SecretField.svelte';
  import Button from '$lib/components/ui/button.svelte';
  import ServiceBrowser from '$lib/components/ServiceBrowser.svelte';
  import Choice from '$lib/components/ui/choice.svelte';
  let credentialField = $state<{ flush: () => Promise<boolean> }>();
  let draft = $state<Config>(structuredClone($state.snapshot(app.data!.config)));
  let editingText = $state(false);
  let selected = $state<Config['ocr']['engine']>(draft.ocr.engine);
  let last = untrack(() => JSON.stringify(draft));
  const autosave = createAutosave<Config>(config => saveConfig(config, { quiet: true }));
  function queue() { const json = JSON.stringify(draft); if (json === last) return; last = json; const snapshot = structuredClone($state.snapshot(draft)); previewConfig(snapshot); autosave.schedule(snapshot); }
  $effect(() => { JSON.stringify(draft); if (!editingText) untrack(queue); });
  onDestroy(() => { queue(); void autosave.flush(); });
  const engines = $derived([
    ['youdao',t("有道 OCR")],
    ['system',app.data?.capabilities.platform === 'macos' ? 'Apple Vision' : app.data?.capabilities.platform === 'linux' ? 'Tesseract' : 'Windows OCR'],
    ['azure','Azure Vision'],
    ['custom',t("自定义 OCR")],
  ] as const);
  let entries = $derived(engines.map(([id,name])=>({id,name,icon:(id==='system'?(app.data?.capabilities.platform==='macos'?'apple':app.data?.capabilities.platform==='linux'?'custom':'windows'):id==='custom'?'custom':id==='azure'?'azure':'youdao') as ProviderBrand,group:id==='system'?t("本机"):id==='custom'?t("自定义"):t("云端"),current:draft.ocr.engine===id})));
  async function select(id:string){if(credentialField && !(await credentialField.flush()))return;selected=id as Config['ocr']['engine'];}
  async function activate() {
    if(credentialField && !(await credentialField.flush())) return;
    draft.ocr.engine=selected;queue();await autosave.flush();
  }

</script>
<div class="service-layout" onfocusin={event=>{if(event.target instanceof HTMLInputElement)editingText=true;}} onfocusout={()=>{editingText=false;queue();}}>
{#if app.configError}<div class="autosave-error" role="alert"><span>{app.configError}</span><button onclick={()=>{autosave.schedule(structuredClone($state.snapshot(draft)));void autosave.flush();}}>{t("重试")}</button></div>{/if}
<ServiceBrowser {entries} {selected} onselect={select} title={t("文字识别服务")}>
  {#snippet header()}<span class="service-protocol">{selected==='system'?t("本机识别"):selected==='custom'?t("自定义接口"):selected==='youdao'?t("免密云端识别"):t("官方云端识别")}</span>{/snippet}
  {#snippet children()}
        {#if selected === 'youdao'}
          <div class="connection-line"><Globe size={17}/><span>{t("选区截图发送至有道")}</span><span class="service-mode">{t("无需密钥")}</span></div>
        {:else if selected === 'system'}
          <div class="connection-line"><Monitor size={17}/><span>{app.data?.capabilities.localOcr ? t("本机识别，无需上传") : t("需要安装识别语言包")}</span><button class="icon-button" aria-label={t("重新检测 OCR")} title={t("重新检测")} onclick={refreshCapabilities}><RefreshCw size={17}/></button><button class="icon-button" aria-label={t("OCR 安装指引")} title={t("安装指引")} onclick={() => act(call('open_help',{topic:'ocr'}))}><ArrowUpRight size={17}/></button></div>
          <label class="field-label" for="ocr-languages">{t("识别语言")}</label><input id="ocr-languages" class="field" value={draft.ocr.languages.join(', ')} onchange={event => draft.ocr.languages = event.currentTarget.value.split(',').map(value => value.trim()).filter(Boolean)}/>
        {:else if selected === 'custom'}
          <div class="provider-form">
            <label class="field-label" for="ocr-protocol">{t("接口协议")}</label><Choice id="ocr-protocol" class="field" label={t("接口协议")} value={draft.ocr.custom.protocol} options={[["openai",t("OpenAI 兼容视觉模型")],["http_json",t("通用 HTTP / JSON")]]} onchange={v => draft.ocr.custom.protocol = v as Config['ocr']['custom']['protocol']}/>
            <label class="field-label" for="custom-ocr-key">{t("API Key · 可选")}</label><SecretField bind:this={credentialField} id="custom-ocr-key" name="custom_ocr" label="OCR API Key"/>
            {#if draft.ocr.custom.protocol === 'openai'}<label class="field-label" for="ocr-model">{t("视觉模型")}</label><div class="model-picker model-manual"><span class="model-input-brand"><ProviderIcon brand={modelBrand(draft.ocr.custom.model,providerBrand("openai",draft.ocr.custom.endpoint))}/></span><input id="ocr-model" class="field" bind:value={draft.ocr.custom.model} placeholder={t("例如 gpt-4.1-mini")} spellcheck="false"/></div>{/if}
            <details class="service-advanced" open={!draft.ocr.custom.endpoint}><summary><ChevronDown size={15}/><span>{t("接口设置")}</span></summary><div class="service-advanced-body">
              <label class="field-label" for="ocr-endpoint">{t("服务地址")}</label><input id="ocr-endpoint" class="field" bind:value={draft.ocr.custom.endpoint} placeholder={draft.ocr.custom.protocol === 'openai' ? 'https://api.example.com/v1' : 'https://api.example.com/ocr'} spellcheck="false"/>
              <details class="protocol-help"><summary>{t("接口格式")}</summary>{#if draft.ocr.custom.protocol === 'http_json'}<p>{t("POST JSON，image 为 PNG Base64，另含 format、width、height、languages。")}</p><pre>{'{"blocks":[{"text":"Hello","bounds":{"x":0,"y":0,"width":100,"height":20}}]}'}</pre>{:else}<p>{t("使用 Chat Completions 图像输入。服务需支持视觉识别与 JSON 输出。")}</p>{/if}<p>{t("坐标单位为请求图片像素。也接受")} {t("{\"text\":\"识别结果\"}")}{t("，作为整图文字块。")}</p></details>
            </div></details>
            <div class="service-disclosure"><Globe size={16}/><span>{t("选区截图发送至填写的服务。")}</span></div>
          </div>
        {:else}
          <div class="provider-form"><label class="field-label" for="azure-endpoint">{t("服务地址")}</label><input id="azure-endpoint" class="field" bind:value={draft.ocr.azureEndpoint} onfocus={() => editingText = true} onblur={() => editingText = false} placeholder={t("https://你的资源.cognitiveservices.azure.com")}/>
            <label class="field-label" for="azure-key">{t("Vision 密钥")}</label><SecretField bind:this={credentialField} id="azure-key" name="azure_ocr" label={t("Vision 密钥")}/>
            <div class="service-disclosure"><Globe size={16}/><span>{t("选区截图发送至 Azure。")}</span></div></div>
        {/if}
<div class="ocr-processing-option"><label for="ocr-upscale">{t("放大小图后识别")}</label><Switch id="ocr-upscale" bind:checked={draft.ocr.upscale}/></div>{/snippet}
  {#snippet footer()}<Button disabled={selected===draft.ocr.engine || (selected==='custom' && (!draft.ocr.custom.endpoint.trim() || (draft.ocr.custom.protocol==='openai' && !draft.ocr.custom.model.trim())))} onclick={activate}>{selected===draft.ocr.engine?t("正在使用"):t("设为当前")}</Button>{/snippet}
</ServiceBrowser></div>
