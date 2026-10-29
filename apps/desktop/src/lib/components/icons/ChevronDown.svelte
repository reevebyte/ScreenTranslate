<script lang="ts">
  import Glyph from './glyph.svelte';
  import type { ComponentProps } from 'svelte';
  let props: Omit<ComponentProps<typeof Glyph>, 'name'> = $props();
</script>
<Glyph name="ChevronDown" {...props}/>
