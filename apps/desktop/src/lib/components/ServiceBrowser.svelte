<script lang="ts">
  import { t } from '$lib/i18n.svelte';
  import type { Snippet } from 'svelte';
  import { prefersReducedMotion } from 'svelte/motion';
  import { Check } from '$lib/icons';
  import type { ProviderBrand } from '$lib/provider-brand';
  import ProviderIcon from './ProviderIcon.svelte';
  import Choice from './ui/choice.svelte';
  import ActiveIndicator from './ui/active-indicator.svelte';
  export type ServiceEntry = {id:string;name:string;group:string;icon:ProviderBrand;current:boolean};
  let {entries,selected,onselect,title,header,children,footer,status}: {entries:ServiceEntry[];selected:string;onselect:(id:string)=>void;title:string;header?:Snippet;children:Snippet;footer:Snippet;status?:Snippet} = $props();
  let detailBody:HTMLDivElement;
  let rendered=false;
  $effect(()=>{
    selected;
    if(!rendered){rendered=true;return;}
    if(prefersReducedMotion.current || !detailBody?.animate)return;
    const style=getComputedStyle(detailBody);
    const animation=detailBody.animate([{opacity:.8},{opacity:1}],{duration:parseFloat(style.getPropertyValue('--motion-fast'))||140,easing:style.getPropertyValue('--ease').trim()||'ease-out'});
    return ()=>animation.cancel();
  });
  let groups=$derived([...new Set(entries.map(item=>item.group))]);
  let entry=$derived(entries.find(item=>item.id===selected));
  let icons=$derived(Object.fromEntries(entries.map(item=>[item.id,item.icon])));
</script>
<div class="service-browser">
  <div class="service-mobile"><Choice class="field" label={title} value={selected} {icons} options={entries.map(item=>[item.id,item.name] as const)} onchange={onselect}/></div>
  <nav class="service-list" aria-label={title}>
    <ActiveIndicator selector='[aria-pressed="true"]'/>
    {#each groups as group}
      <div class="service-list-group"><h2>{group}</h2>
        {#each entries.filter(item=>item.group===group) as item}
          <button class="service-list-item" title={t(item.name)} aria-label={t(item.name)} aria-pressed={item.id===selected} onclick={()=>onselect(item.id)}><ProviderIcon brand={item.icon}/><span class="service-name">{t(item.name)}</span>{#if item.current}<Check size={14} class="service-in-use"/><span class="sr-only">{t("使用中")}</span>{/if}</button>
        {/each}
      </div>
    {/each}
  </nav>
  <section class="service-detail" aria-label={t("服务配置")}>
    <header class="service-detail-heading"><ProviderIcon brand={entry?.icon} featured/><div class="service-heading-text"><h2>{t(entry?.name??'')}</h2>{@render header?.()}</div></header>
    <div class="service-detail-body" bind:this={detailBody}>{@render children()}</div>
    <footer class="service-detail-actions"><div class="service-feedback" role="status" aria-live="polite">{@render status?.()}</div><div class="service-buttons">{@render footer()}</div></footer>
  </section>
</div>
