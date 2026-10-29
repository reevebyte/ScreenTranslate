<script lang="ts">
  import { t } from '$lib/i18n.svelte';
  import { onDestroy } from 'svelte';
  import { Command, Popover } from 'bits-ui';
  import { Check, ChevronDown, LoaderCircle, RefreshCw, Search } from '$lib/icons';
  import ProviderIcon from './ProviderIcon.svelte';
  import { modelBrand, type ProviderBrand } from '$lib/provider-brand';
  import { errorMessage } from '$lib/utils';

  let { value = $bindable(''), identity, load, id = 'provider-model', brand='custom' }:
    { value?: string; identity: string; load: () => Promise<string[]>; id?: string; brand?:ProviderBrand } = $props();
  let open = $state(false);
  let models = $state<string[]>([]);
  let loading = $state(false);
  let loaded = $state(false);
  let error = $state('');
  let query = $state('');
  let generation = 0;
  let alive = true;
  $effect(() => {
    identity;
    generation++;
    models = []; loaded = false; loading = false; error = ''; query = ''; open = false;
  });
  onDestroy(() => { alive = false; generation++; });
  async function refresh() {
    if (loading) return;
    const request = ++generation, origin = identity;
    loading = true; error = '';
    try {
      const results = await load();
      if (!alive || request !== generation || origin !== identity) return;
      models = [...new Set(results.map(item => item.trim()).filter(Boolean))];
      loaded = true;
    } catch (cause) {
      if (alive && request === generation && origin === identity) error = errorMessage(cause);
    } finally {
      if (alive && request === generation && origin === identity) loading = false;
    }
  }
  function changeOpen(next: boolean) {
    open = next;
    if (next && !loaded && !loading) void refresh();
  }
</script>

<div class="model-picker">
  <span class="model-input-brand"><ProviderIcon brand={modelBrand(value,brand)}/></span>
  <input {id} class="field" bind:value placeholder={t("输入模型 ID，或从列表选择")} spellcheck="false"/>
  <Popover.Root {open} onOpenChange={changeOpen}>
    <Popover.Trigger class="icon-button model-picker-toggle" aria-label={t("选择模型")} title={t("选择模型")}><ChevronDown size={16}/></Popover.Trigger>
    <Popover.Portal>
      <Popover.Content class="model-menu" sideOffset={6} collisionPadding={12} align="end" aria-label={t("模型列表")}>
        <Command.Root class="model-command">
          <div class="model-search"><Search size={16}/><Command.Input bind:value={query} placeholder={t("搜索模型…")} aria-label={t("搜索模型")}/><button class="icon-button" aria-label={t("刷新模型列表")} title={t("刷新模型列表")} disabled={loading} onclick={refresh}>{#if loading}<LoaderCircle size={16} class="spin"/>{:else}<RefreshCw size={16}/>{/if}</button></div>
          <Command.List class="model-list" aria-busy={loading}>
            {#if loading}<p class="model-message" role="status">{t("正在获取模型…")}</p>
            {:else if error}<div class="model-message error-text" role="alert"><span>{error}</span><button class="text-link" onclick={refresh}>{t("重试")}</button></div>
            {:else if loaded && !models.length}<p class="model-message">{t("没有返回模型，可直接输入模型 ID。")}</p>
            {:else}
              <Command.Empty class="model-message">{t("没有匹配的模型，可直接输入。")}</Command.Empty>
              {#each models as model}<Command.Item value={model} class="model-option" onSelect={() => { value = model; open = false; }}><ProviderIcon brand={modelBrand(model,brand)}/><span>{model}</span>{#if value === model}<Check size={15}/>{/if}</Command.Item>{/each}
            {/if}
          </Command.List>
        </Command.Root>
      </Popover.Content>
    </Popover.Portal>
  </Popover.Root>
</div>
