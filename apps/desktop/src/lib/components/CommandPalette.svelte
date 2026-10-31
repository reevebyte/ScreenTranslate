<script lang="ts">
  import { t } from '$lib/i18n.svelte';
  import { Command, Dialog } from 'bits-ui';
  import { Search, ScanLine, Languages, ImagePlus, Settings2, PanelsTopLeft, Keyboard } from '$lib/icons';
  import { app, act, type Route } from '$lib/store.svelte';
  import { call } from '$lib/api';

  const actions = $derived([
    { label: t("截图翻译"), keywords: ['capture', t("截图")], icon: ScanLine, action: () => act(call('begin_capture', { monitor: null })), shortcut: app.data?.config.hotkeys.capture || '' },
    { label: t("文字翻译"), keywords: ['translate', t("文字")], icon: Languages, action: () => navigate('translate'), shortcut: '' },
    { label: t("打开图片"), keywords: ['image', t("图片")], icon: ImagePlus, action: () => act(call('open_image')), shortcut: '' },
    { label: t("快速翻译框"), keywords: ['quick', t("浮窗")], icon: PanelsTopLeft, action: () => act(call('toggle_quick')), shortcut: app.data?.config.hotkeys.quick || '' },
    { label: t("服务中心"), keywords: ['provider', 'api', t("模型"), 'OCR'], icon: Keyboard, action: () => navigate('providers'), shortcut: '' },
    { label: t("快捷操作"), keywords: ['hotkeys', t("快捷键")], icon: Keyboard, action: () => navigate('shortcuts'), shortcut: '' },
    { label: t("设置"), keywords: ['settings', t("外观"), t("快捷键")], icon: Settings2, action: () => navigate('settings'), shortcut: '' },
  ]);
  function navigate(route: Route) { app.route = route; }
  function run(action: () => unknown) { app.commandOpen = false; action(); }
</script>

<Dialog.Root bind:open={app.commandOpen}>
  <Dialog.Portal>
    <Dialog.Overlay class="dialog-overlay" />
    <Dialog.Content class="command-dialog">
      <Dialog.Title class="sr-only">{t("搜索命令")}</Dialog.Title>
      <Dialog.Description class="sr-only">{t("输入功能名称，用方向键选择并按回车执行。")}</Dialog.Description>
      <Command.Root class="command-root">
        <div class="command-search"><Search size={19}/><Command.Input placeholder={t("搜索功能、设置或操作…")} class="command-input"/><kbd>Esc</kbd></div>
        <Command.List class="command-list">
          <Command.Empty class="command-empty">{t("没有找到相关操作")}</Command.Empty>
          <Command.Group>
            <Command.GroupHeading class="command-heading">{t("常用操作")}</Command.GroupHeading>
            {#each actions as action}
              <Command.Item value={action.label} keywords={action.keywords} onSelect={() => run(action.action)} class="command-item">
                <span class="setting-row-icon" aria-hidden="true"><action.icon size={16}/></span><span>{action.label}</span>{#if action.shortcut}<small>{action.shortcut}</small>{/if}
              </Command.Item>
            {/each}
          </Command.Group>
        </Command.List>
        <footer class="command-footer"><span><kbd>↑</kbd><kbd>↓</kbd> {t("选择")}</span><span><kbd>↵</kbd> {t("执行")}</span></footer>
      </Command.Root>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
