<script lang="ts">
  import { t } from '$lib/i18n.svelte';
  import { Tooltip } from 'bits-ui';
  import { Check, TriangleAlert } from '$lib/icons';
  let { result }: { result: { ok: boolean; detail: string } | null } = $props();
</script>
{#if result}
  <Tooltip.Provider delayDuration={150}><Tooltip.Root>
    <Tooltip.Trigger class={`connection-status ${result.ok ? 'success' : 'error-text'}`} aria-label={`${result.ok ? t("连接成功") : t("连接失败")}：${result.detail}`}>
      {#if result.ok}<Check size={15}/>{:else}<TriangleAlert size={15}/>{/if}<span>{result.ok ? t("连接成功") : t("连接失败，查看详情")}</span>
    </Tooltip.Trigger>
    <Tooltip.Portal><Tooltip.Content role="tooltip" class="tooltip-content connection-details" sideOffset={7} collisionPadding={12}>{result.detail}</Tooltip.Content></Tooltip.Portal>
  </Tooltip.Root></Tooltip.Provider>
{/if}
