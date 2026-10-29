<script lang="ts">
  import type { HTMLButtonAttributes } from 'svelte/elements';
  import type { Snippet } from 'svelte';
  import { cn } from '$lib/utils';
  import { Button as ShadcnButton } from './button/index';
  let { variant = 'default', size = 'default', class: className, children, ...rest }:
    HTMLButtonAttributes & { variant?: 'default' | 'secondary' | 'ghost' | 'outline' | 'danger'; size?: 'default' | 'sm' | 'icon'; children?: Snippet } = $props();
</script>

<ShadcnButton variant={variant === 'danger' ? 'destructive' : variant} {size} class={cn('button', `button-${variant}`, `button-size-${size}`, className)} {...rest}>
  {@render children?.()}
</ShadcnButton>
