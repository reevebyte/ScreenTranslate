<script lang="ts">
  import type { SVGAttributes } from 'svelte/elements';
  import { paths } from './paths';
  type Props = Omit<SVGAttributes<SVGSVGElement>, 'color'> & { name: keyof typeof paths; size?: number | string; color?: string };
  let { name, size = 20, color = 'currentColor', ...props }: Props = $props();
</script>
<svg xmlns="http://www.w3.org/2000/svg" width={size} height={size} viewBox="0 0 24 24" fill="none" stroke={color} stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" {...props}><path d={paths[name]}/></svg>
