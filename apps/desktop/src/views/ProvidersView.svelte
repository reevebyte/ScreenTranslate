<script lang="ts">
  import { t } from '$lib/i18n.svelte';
  import { Globe2, LoaderCircle, PlugZap, ChevronDown } from '$lib/icons';
  import { toast } from 'svelte-sonner';
  import { onDestroy, untrack } from 'svelte';
  import { app, saveConfig, previewConfig } from '$lib/store.svelte';
  import { createAutosave } from '$lib/autosave';
  import { call } from '$lib/api';
  import { providers, providerInfo } from '$lib/providers';
  import { providerBrand } from '$lib/provider-brand';
  import { errorMessage } from '$lib/utils';
  import type { Config, ProviderId } from '$lib/generated';
  import Button from '$lib/components/ui/button.svelte';
  import SecretField from '$lib/components/SecretField.svelte';
  import Choice from '$lib/components/ui/choice.svelte';
  import ServiceBrowser from '$lib/components/ServiceBrowser.svelte';
  import ProviderIcon from '$lib/components/ProviderIcon.svelte';
  import ModelPicker from '$lib/components/ModelPicker.svelte';
  import ConnectionStatus from '$lib/components/ConnectionStatus.svelte';
  let draft = $state<Config>(structuredClone($state.snapshot(app.data!.config)));
  let selected = $state<ProviderId>(app.data!.config.provider);
  let persistent = $state(true); let testing = $state(false); let presetName = $state('custom');
  let credentialVersion = $state(0);
  let result = $state<{ok:boolean;detail:string}|null>(null);
  let testRequest = 0, alive = true;
  let credentialField = $state<{flush:()=>Promise<boolean>}>();
  let info = $derived(providerInfo(selected)); let options = $derived(draft.providers[selected]!);
  let ai = $derived(['openai','nvidia','anthropic'].includes(selected));
  let selectedBrand=$derived(providerBrand(selected,options.baseUrl));
  let entries=$derived(providers.slice().sort((a,b)=>Number(!!a.unofficial)-Number(!!b.unofficial)).map(p=>({id:p.id,name:p.id==='openai'?t("OpenAI 兼容"):p.id==='google_free'?t("Google 网页"):p.id==='microsoft'?'Microsoft':t(p.name),group:p.unofficial?t("免费接口"):t("API 服务"),icon:p.id==='openai'?'openai' as const:providerBrand(p.id,draft.providers[p.id]?.baseUrl),current:draft.provider===p.id})));
  let editing = $state(false); let last = untrack(()=>JSON.stringify(draft));
  const writer=createAutosave<Config>(config=>saveConfig(config,{quiet:true}));
  function queue(){const json=JSON.stringify(draft);if(json===last)return;last=json;const value=structuredClone($state.snapshot(draft));previewConfig(value);writer.schedule(value);}
  $effect(()=>{JSON.stringify(draft);if(!editing)untrack(queue);});
  let identity = $derived(JSON.stringify([selected, options.baseUrl, credentialVersion]));
  let testIdentity = $derived(JSON.stringify([identity, options]));
  onDestroy(()=>{alive=false;testRequest++;queue();void writer.flush();});
  $effect(()=>{testIdentity;testRequest++;result=null;testing=false;});
  async function select(id:string){if(credentialField && !(await credentialField.flush()))return;selected=id as ProviderId;presetName='custom';}
  async function save(current=true){if(credentialField && !(await credentialField.flush()))return false;if(current)draft.provider=selected;queue();return writer.flush();}
  async function preset(value:string){
    if(credentialField && !(await credentialField.flush()))return;
    presetName=value;
    if(value==='nvidia'){selected='nvidia';return;}
    if(value==='custom')return;
    selected='openai';
    const presets:Record<string,[string,string]>={openai:['https://api.openai.com/v1','gpt-4.1-mini'],deepseek:['https://api.deepseek.com/v1','deepseek-chat'],ollama:['http://localhost:11434/v1','']};
    const chosen=presets[value];if(chosen){draft.providers.openai!.baseUrl=chosen[0];draft.providers.openai!.model=chosen[1];}
  }
  async function test(){
    if(testing)return;
    const provider=selected,origin=testIdentity,request=++testRequest;
    testing=true;result=null;
    const current=()=>alive&&request===testRequest&&origin===testIdentity;
    try{
      if(!(await save(false))){if(current())result={ok:false,detail:app.configError||t("设置未保存，请重试。")};return;}
      if(!current())return;
      const detail=await call<string>('test_provider',{provider});
      if(current())result={ok:true,detail};
    }catch(error){if(current())result={ok:false,detail:errorMessage(error)};}
    finally{if(current())testing=false;}
  }
  async function fetchModels(){
    const provider=selected,origin=identity;
    if(!(await save(false)))throw new Error(app.configError||t("请先保存服务设置"));
    if(origin!==identity||!alive)return [];
    return call<string[]>('list_models',{provider});
  }
</script>
<div class="service-layout" onfocusin={event=>{if(event.target instanceof HTMLInputElement && event.target.type!=='checkbox')editing=true;}} onfocusout={()=>{editing=false;queue();}}>
  {#if app.configError}<div class="autosave-error" role="alert"><span>{app.configError}</span><button onclick={()=>{writer.schedule(structuredClone($state.snapshot(draft)));void writer.flush();}}>{t("重试")}</button></div>{/if}
  <ServiceBrowser {entries} {selected} onselect={select} title={t("翻译服务")}>
    {#snippet header()}{#if info.unofficial}<span class="service-protocol">{t("非官方接口")}</span>{/if}{/snippet}
    {#snippet children()}
      {#if info.unofficial}<div class="service-free-description"><span class="setting-row-icon" aria-hidden="true"><Globe2 size={17}/></span><div><strong>{t("无需密钥")}</strong><p>{t("识别后的文字发送至")}{t(info.name)}。</p></div></div>
      {:else}<div class="provider-form">
        <label class="field-label" for="provider-secret">API Key{selected==='openai'?t(" · 可选"):''}</label>{#key selected}<SecretField bind:this={credentialField} id="provider-secret" name={selected} label="API Key" {persistent} onchange={()=>credentialVersion++}/>{/key}
        {#if ai}<label class="field-label" for="provider-model">{t("模型")}</label>{#if selected==='anthropic'}<div class="model-picker model-manual"><span class="model-input-brand"><ProviderIcon brand={selectedBrand}/></span><input id="provider-model" class="field" bind:value={options.model} placeholder={t("输入模型 ID")} spellcheck="false"/></div>{:else}<ModelPicker bind:value={options.model} {identity} brand={selectedBrand} load={fetchModels}/>{/if}{/if}
        <details class="service-advanced"><summary><ChevronDown size={15}/><span>{t("接口设置")}</span></summary>
          <div class="service-advanced-body">
{#if selected==='openai'}<label class="field-label" for="provider-preset">{t("服务预设")}</label><Choice id="provider-preset" class="field" icons={{custom:"custom",openai:"openai",deepseek:"deepseek",ollama:"ollama"}} label={t("服务预设")} value={presetName} options={[["custom",t("自定义")],["openai","OpenAI"],["deepseek","DeepSeek"],["ollama",t("Ollama 本地")]]} onchange={value=>void preset(value)}/>{/if}
          <label class="field-label" for="provider-endpoint">{t("服务地址")}</label><input id="provider-endpoint" class="field" bind:value={options.baseUrl} placeholder="https://…" spellcheck="false"/>
          {#if selected==='microsoft'}<label class="field-label" for="provider-region">Region</label><input id="provider-region" class="field" bind:value={options.region} placeholder={t("例如 eastasia")} spellcheck="false"/>{/if}
          {#if selected==='deepl'}<label class="checkbox-label"><input type="checkbox" bind:checked={options.freePlan} onchange={event=>{options.freePlan=event.currentTarget.checked;options.baseUrl=event.currentTarget.checked?'https://api-free.deepl.com/v2':'https://api.deepl.com/v2';}}/>{t("使用 DeepL API Free")}</label>{/if}
          <label class="checkbox-label"><input type="checkbox" bind:checked={persistent}/>{t("保存密钥")}</label>
          </div>
        </details>
      </div>{/if}
    {/snippet}
    {#snippet status()}<ConnectionStatus {result}/>{/snippet}
    {#snippet footer()}<Button variant="outline" disabled={testing} onclick={test}>{#if testing}<LoaderCircle size={16} class="spin"/>{:else}<PlugZap size={16}/>{/if}{testing?t("检测中…"):t("测试连接")}</Button><Button disabled={draft.provider===selected} onclick={async()=>{if(await save())toast.success(t("已设为当前翻译服务"));}}>{draft.provider===selected?t("正在使用"):t("设为当前")}</Button>{/snippet}
  </ServiceBrowser>
</div>
