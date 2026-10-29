<script lang="ts">
  import { t } from '$lib/i18n.svelte';
  import { LoaderCircle, Check, CircleDashed, TriangleAlert } from '$lib/icons';
  import type { JobStatus } from '$lib/generated';
  let { job }: { job: JobStatus } = $props();
  const labels = $derived({ idle: t("等待输入"), capturing: t("正在截图"), recognizing: t("正在识别文字"), translating: t("正在翻译"), complete: t("翻译完成"), cancelled: t("已取消"), error: t("需要处理") });
  let busy = $derived(['capturing', 'recognizing', 'translating'].includes(job.stage));
</script>
<span class="job-indicator" class:success={job.stage === 'complete'} class:error={job.stage === 'error'} role="status" aria-live="polite">
  {#if busy}<LoaderCircle size={13} class="spin" />{:else if job.stage === 'complete'}<Check size={13}/>{:else if job.stage === 'error'}<TriangleAlert size={13}/>{:else}<CircleDashed size={13}/>{/if}
  {labels[job.stage]}
</span>
