<script lang="ts">
  import * as Select from './select/index';
  import ProviderIcon from '../ProviderIcon.svelte';
  import type { ProviderBrand } from '$lib/provider-brand';
  import { cn } from '$lib/utils';
  let { value, options, label, onchange, class: className, id, icons }:
    { value: string; options: ReadonlyArray<readonly [string, string]>; label: string; onchange: (value: string) => void; class?: string; id?: string; icons?: Record<string,ProviderBrand> } = $props();
</script>

<Select.Root type="single" {value} onValueChange={onchange}>
  <Select.Trigger {id} class={cn('choice-trigger', className)} aria-label={label} title={options.find(([key]) => key === value)?.[1] || label}>
    <span class="choice-value">{#if icons?.[value]}<ProviderIcon brand={icons[value]}/>{/if}<span>{options.find(([key]) => key === value)?.[1] || label}</span></span>
  </Select.Trigger>
  <Select.Content side="bottom" align="start" collisionPadding={8} class="choice-menu">
    <Select.Group>
      {#each options as [key, text]}<Select.Item value={key} label={text}>{#if icons?.[key]}<ProviderIcon brand={icons[key]}/>{/if}{text}</Select.Item>{/each}
    </Select.Group>
  </Select.Content>
</Select.Root>
