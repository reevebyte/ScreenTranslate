<script lang="ts">
  import { onDestroy } from 'svelte';
  import { Check } from '$lib/icons';
  import { t } from '$lib/i18n.svelte';
  import { copy } from '$lib/store.svelte';
  let { text, label, class: className = 'text-action' }: { text: string; label: string; class?: string } = $props();
  let copied = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;
  let pending = false;
  async function handleCopy() {
    if (pending) return;
    pending = true;
    try {
      if (!await copy(text)) return;
      clearTimeout(timer);
      copied = true;
      timer = setTimeout(() => copied = false, 1400);
    } finally { pending = false; }
  }
  onDestroy(() => clearTimeout(timer));
</script>
<button class={`copy-feedback-button ${className}`} aria-label={label} title={copied ? t('已复制') : label} onclick={handleCopy}>{#if copied}<Check size={17} class="copy-done"/>{:else}{t('复制')}{/if}</button>
