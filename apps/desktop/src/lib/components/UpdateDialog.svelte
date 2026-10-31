<script lang="ts">
 import { Dialog } from 'bits-ui';
 import { updates, installUpdate, updateBlockReason } from '$lib/updater.svelte';
 import { t } from '$lib/i18n.svelte';
 import { X } from '$lib/icons';
 import Button from './ui/button.svelte';
 const blockReason=$derived(updateBlockReason());
</script>
<Dialog.Root bind:open={updates.confirm}>
  <Dialog.Portal><Dialog.Overlay class="dialog-overlay"/><Dialog.Content class="reading-dialog update-dialog" onEscapeKeydown={event=>{if(updates.stage==='installing')event.preventDefault();}} onInteractOutside={event=>{if(updates.stage==='installing')event.preventDefault();}}>
    <div class="dialog-heading"><Dialog.Title>{t('重启更新')}</Dialog.Title><Dialog.Close class="icon-button" disabled={updates.stage==='installing'} aria-label={t('关闭')}><X size={18}/></Dialog.Close></div>
    <Dialog.Description>{t('应用将重启，当前文字和截图会清空，设置将保留。')}</Dialog.Description>
    <p class="update-dialog-version">ScreenTranslate {updates.version}</p>
    {#if updates.notes}<div class="update-release-notes">{updates.notes}</div>{/if}
    {#if updates.error || blockReason}<p class="error-text" role="alert">{updates.error || blockReason}</p>{/if}
    <div class="dialog-actions"><Dialog.Close class="button button-outline button-size-default" disabled={updates.stage==='installing'}>{t('稍后')}</Dialog.Close><Button disabled={updates.stage==='installing' || !!blockReason} onclick={installUpdate}>{t('重启更新')}</Button></div>
  </Dialog.Content></Dialog.Portal>
</Dialog.Root>
