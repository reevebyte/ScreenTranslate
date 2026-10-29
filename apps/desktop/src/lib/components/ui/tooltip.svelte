<script lang="ts">
  import { Tooltip } from 'bits-ui';
  import type { Snippet } from 'svelte';
  let { text, children }: { text: string; children: Snippet } = $props();
</script>

<Tooltip.Provider delayDuration={350}>
  <Tooltip.Root>
    <Tooltip.Trigger class="tooltip-trigger" aria-label={text}>{@render children()}</Tooltip.Trigger>
    <Tooltip.Portal><Tooltip.Content class="tooltip-content" sideOffset={7}>{text}</Tooltip.Content></Tooltip.Portal>
  </Tooltip.Root>
</Tooltip.Provider>
