<script lang="ts">
  import { t } from '$lib/i18n.svelte';
  import { Tabs } from 'bits-ui';
  import { app } from '$lib/store.svelte';
  import { Languages, ScanLine } from '$lib/icons';
  import ProvidersView from './ProvidersView.svelte';
  import OcrSettings from './OcrSettings.svelte';
  import ActiveIndicator from '$lib/components/ui/active-indicator.svelte';
</script>
<div class="service-center">
  <Tabs.Root bind:value={app.servicesTab} class="service-tabs">
    <Tabs.List class="service-tab-list" aria-label={t("服务类型")}>
      <ActiveIndicator/>
      <Tabs.Trigger value="translation"><Languages size={18}/>{t("翻译接口")}</Tabs.Trigger>
      <Tabs.Trigger value="ocr"><ScanLine size={18}/>{t("文字识别")}</Tabs.Trigger>
    </Tabs.List>
    <Tabs.Content value="translation" class="service-content">{#if app.servicesTab === 'translation'}<ProvidersView/>{/if}</Tabs.Content>
    <Tabs.Content value="ocr" class="service-content">{#if app.servicesTab === 'ocr'}<OcrSettings/>{/if}</Tabs.Content>
  </Tabs.Root>
</div>
