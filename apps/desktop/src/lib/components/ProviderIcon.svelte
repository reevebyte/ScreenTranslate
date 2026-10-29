<script lang="ts">
  import { PlugZap } from '$lib/icons';
  import type { ProviderBrand } from '$lib/provider-brand';
  import openai from '$lib/assets/providers/openai.svg?raw';
  import claude from '$lib/assets/providers/claude-color.svg?raw';
  import google from '$lib/assets/providers/google-color.svg?raw';
  import googlecloud from '$lib/assets/providers/googlecloud-color.svg?raw';
  import azure from '$lib/assets/providers/azure-color.svg?raw';
  import microsoft from '$lib/assets/providers/microsoft-color.svg?raw';
  import nvidia from '$lib/assets/providers/nvidia-color.svg?raw';
  import deepseek from '$lib/assets/providers/deepseek-color.svg?raw';
  import ollama from '$lib/assets/providers/ollama.svg?raw';
  import bing from '$lib/assets/providers/bing-color.svg?raw';
  import tencent from '$lib/assets/providers/tencent-color.svg?raw';
  import yandex from '$lib/assets/providers/yandex.svg?raw';
  import deepl from '$lib/assets/providers/deepl.svg?raw';
  import apple from '$lib/assets/providers/apple.svg?raw';
  import meta from '$lib/assets/providers/meta-color.svg?raw';
  import qwen from '$lib/assets/providers/qwen-color.svg?raw';
  import mistral from '$lib/assets/providers/mistral-color.svg?raw';
  import zhipu from '$lib/assets/providers/zhipu-color.svg?raw';
  import youdao from '$lib/assets/providers/youdao.ico';
  import iciba from '$lib/assets/providers/iciba.ico';
  let {brand='custom',featured=false}: {brand?:ProviderBrand;featured?:boolean}=$props();
  const componentId=$props.id();
  const instance=componentId.replace(/[^a-zA-Z0-9_-]/g,'');
  const artwork:Partial<Record<ProviderBrand,string>>={openai,claude,google,googlecloud,azure,microsoft,nvidia,deepseek,ollama,bing,tencent,yandex,deepl,apple,meta,qwen,mistral,zhipu};
  let svg=$derived((artwork[brand]??'').replace(/\bid="([^"]+)"/g,(_,id)=>`id="${instance}-${id}"`).replace(/url\(#([^)]+)\)/g,(_,id)=>`url(#${instance}-${id})`).replace(/\b((?:xlink:)?href)="#([^"]+)"/g,(_,attr,id)=>`${attr}="#${instance}-${id}"`));
</script>
<span class="provider-icon" class:provider-icon-featured={featured} aria-hidden="true" data-brand={brand}>
  {#if brand==='youdao'||brand==='iciba'}<img src={brand==='youdao'?youdao:iciba} alt=""/>
  {:else if brand==='windows'}<svg viewBox="0 0 24 24" fill="#0078d4"><path d="M2 2h9v9H2zm11 0h9v9h-9zM2 13h9v9H2zm11 0h9v9h-9z"/></svg>
  {:else if artwork[brand]}{@html svg}
  {:else}<PlugZap size={20}/>{/if}
</span>
