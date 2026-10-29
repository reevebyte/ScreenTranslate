<script lang="ts">
  import type { ComponentProps, Snippet } from 'svelte';
  import * as Tooltip from './tooltip/index';
  import { cn } from '$lib/utils';
  let { label, active = false, children, class: className, ...rest }:
    Omit<ComponentProps<typeof Tooltip.Trigger>, 'child' | 'children'> & { label: string; active?: boolean; children: Snippet } = $props();
</script>

<Tooltip.Provider delayDuration={400}>
  <Tooltip.Root>
    <Tooltip.Trigger class={cn('icon-button', active && 'active', className)} aria-label={label} {...rest}>
      {@render children()}
    </Tooltip.Trigger>
    <Tooltip.Content side="right" class="text-sm">{label}</Tooltip.Content>
  </Tooltip.Root>
</Tooltip.Provider>
