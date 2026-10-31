<script lang="ts">
  import { t } from '$lib/i18n.svelte';
  import { app, act } from '$lib/store.svelte';
  import { call } from '$lib/api';
  import { updates, checkUpdate, downloadUpdate, updateBlockReason } from '$lib/updater.svelte';
  import Button from '$lib/components/ui/button.svelte';
  import Switch from '$lib/components/ui/switch/switch.svelte';
  import { ArrowUpRight, Code2, CircleHelp, Keyboard, Check, LoaderCircle, RefreshCw } from '$lib/icons';
  let { onwelcome, autoUpdate = $bindable(true) }: { onwelcome?: () => void; autoUpdate?: boolean } = $props();
  const waiting = $derived(['checking','downloading','installing'].includes(updates.stage));
  const blockReason = $derived(updateBlockReason());
  const status = $derived(updates.stage === 'installing' ? t('安装中…') : updates.stage === 'checking' ? t('检查中…') : updates.stage === 'current' ? t('已是最新版本') : updates.stage === 'downloading' ? t('正在下载 {0}',updates.progress === null ? '…' : updates.progress + '%') : updates.stage === 'ready' ? t('更新已就绪') : updates.version ? t('发现新版本 {0}',updates.version) : '');
</script>
<div class="about-view">
  <header class="about-identity">
    <div class="about-icon"><img src="/app-icon.png" alt={t('ScreenTranslate 图标')} width="52" height="52"/></div>
    <h2>ScreenTranslate</h2>
    <span class="about-version">v{app.data?.version}</span>
    <div class="about-update-actions" aria-label={t('软件更新')}>
      {#if updates.stage === 'ready' || updates.stage === 'installing'}<Button variant="outline" disabled={waiting || !!blockReason} onclick={()=>updates.confirm=true}><RefreshCw size={16}/>{updates.stage === 'installing'?t('安装中…'):t('重启更新')}</Button>
      {:else if app.data?.updaterReady}<Button variant="outline" disabled={waiting} onclick={()=>updates.version && ['available','error'].includes(updates.stage) ? downloadUpdate() : checkUpdate()}>{#if waiting}<LoaderCircle size={16} class="spin"/>{:else}<RefreshCw size={16}/>{/if}{updates.stage === 'downloading'?t('下载中…'):updates.version && ['available','error'].includes(updates.stage)?t('下载更新'):updates.stage === 'checking'?t('检查中…'):t('检查更新')}</Button>
      {:else}<Button variant="outline" onclick={()=>act(call('open_help',{topic:'releases'}))}>{t('查看发布')}<ArrowUpRight size={16}/></Button>{/if}
    </div>
    <div class="about-update-status-slot">
      {#if updates.error}<p class="about-update-status error-text" role="alert">{updates.error}</p>{:else if status}<p class="about-update-status" role="status">{#if updates.stage==='current' || updates.stage==='ready'}<Check size={16}/>{/if}{status}</p>{/if}
      {#if updates.stage==='ready' && blockReason}<p class="about-update-status" role="status">{blockReason}</p>{/if}
    </div>
  </header>
  <div class="about-resources">
    <div class="about-resource about-update-preference"><span class="about-resource-icon"><RefreshCw size={19}/></span><label for="auto-update">{t('自动检查更新')}</label><Switch id="auto-update" bind:checked={autoUpdate}/></div>
    <button class="about-resource" onclick={()=>act(call('open_help',{topic:'repository'}))}><span class="about-resource-icon"><Code2 size={19}/></span><span>{t('项目主页')}</span><ArrowUpRight size={17}/></button>
    <button class="about-resource" onclick={()=>act(call('open_help',{topic:'issues'}))}><span class="about-resource-icon"><CircleHelp size={19}/></span><span>{t('反馈问题')}</span><ArrowUpRight size={17}/></button>
    {#if onwelcome}<button class="about-resource" onclick={onwelcome}><span class="about-resource-icon"><Keyboard size={19}/></span><span>{t('使用指南')}</span><ArrowUpRight size={17}/></button>{/if}
  </div>
</div>
