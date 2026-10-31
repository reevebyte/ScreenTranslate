<script lang="ts">
  import { t, initializeLocale } from '$lib/i18n.svelte';
  import { updates, scheduleAutomaticCheck, initializeUpdates, syncUpdateUi, consumeUpdateRequest } from '$lib/updater.svelte';
  import { onMount, tick } from 'svelte';
  import ActiveIndicator from '$lib/components/ui/active-indicator.svelte';
  import { listen } from '@tauri-apps/api/event';
  import { Toaster } from 'svelte-sonner';
  import { LoaderCircle, Languages, Image, SlidersHorizontal, Settings2, Keyboard, PanelsTopLeft, Download, X } from '$lib/icons';
  import { app, initialize, applyAppearance, type Route } from '$lib/store.svelte';
  import { call, desktop } from '$lib/api';
  import Brand from '$lib/components/Brand.svelte';
  import Button from '$lib/components/ui/button.svelte';
  import TextView from './views/TextView.svelte';
  import ResultView from './views/ResultView.svelte';
  import CaptureOverlay from './views/CaptureOverlay.svelte';
  import QuickView from './views/QuickView.svelte';
  import ServicesView from './views/ServicesView.svelte';
  import ShortcutsView from './views/ShortcutsView.svelte';
  import SettingsView from './views/SettingsView.svelte';
  import UpdateDialog from '$lib/components/UpdateDialog.svelte';
  const mode = new URLSearchParams(location.search).get('window') || 'main';
  const nav = $derived([
    { route: 'shortcuts' as Route, label: t("快捷操作"), icon: Keyboard },
    { route: 'translate' as Route, label: t("文字翻译"), icon: Languages },
    { route: 'capture' as Route, label: t("截图结果"), icon: Image },
    { route: 'providers' as Route, label: t("服务中心"), icon: SlidersHorizontal },
    { route: 'settings' as Route, label: t("设置"), icon: Settings2 },
  ]);
  function storedSidebar() { try { return localStorage.getItem('screentranslate-rail-collapsed') !== 'false'; } catch { return true; } }
  let sidebarCollapsed = $state(storedSidebar());
  function toggleSidebar() {
    sidebarCollapsed = !sidebarCollapsed;
    try { localStorage.setItem('screentranslate-rail-collapsed', String(sidebarCollapsed)); } catch { /* Storage may be disabled. */ }
  }
  let welcome = $state(false); let offeredWelcome = false;
  $effect(() => { if (app.data) applyAppearance(app.data.config); });
  $effect(() => { if (desktop && mode === 'main' && app.data && app.route !== 'components') void call('set_route', { route: app.route }).catch(() => {}); });
  $effect(() => { if (mode === 'main' && app.data && !app.data.config.onboardingComplete && !offeredWelcome) { offeredWelcome = true; welcome = true; } });
  $effect(() => { if(mode==='main')syncUpdateUi(); });
  $effect(() => { if(mode==='main' && app.data && updates.connected)void consumeUpdateRequest(); });
  onMount(initializeLocale);
  onMount(() => mode === 'main' ? initializeUpdates() : undefined);
  $effect(() => { if (mode === 'main') scheduleAutomaticCheck(!!app.data?.updaterReady && !!app.data?.config.autoUpdate && !!app.data?.config.onboardingComplete); });
  onMount(() => {
    document.body.dataset.window = mode;
    let cleanup: (() => void) | undefined;
    let navigateCleanup: (() => void) | undefined;
    let disposed = false;
    if (desktop) void listen<string>('navigate', ({ payload }) => { if (payload === 'ocr') { app.servicesTab = 'ocr'; app.route = 'providers'; } else if (nav.some(item => item.route === payload)) { if (payload === 'providers') app.servicesTab = 'translation'; app.route = payload as Route; } }).then(fn => { if (disposed) fn(); else navigateCleanup = fn; });
    void initialize().then(async fn => {
      if (disposed) fn(); else cleanup = fn;
      if (import.meta.env.DEV && new URLSearchParams(location.search).has('gallery')) app.route = 'components';
      if (desktop && !disposed && (mode === 'main' || mode === 'quick' || mode === 'result-tools' || app.fatal)) {
        await tick();
        await document.fonts?.ready;
        await new Promise(resolve => requestAnimationFrame(resolve));
        if (!disposed) void call('window_ready', { frameId: null }).catch(() => {});
      }
    });
    const media = matchMedia('(prefers-color-scheme: dark)');
    const themeChange = () => { if (app.data) applyAppearance(app.data.config); };
    media.addEventListener('change', themeChange);
    return () => { disposed = true; cleanup?.(); navigateCleanup?.(); media.removeEventListener('change', themeChange); };
  });
  function keydown(event: KeyboardEvent) {
    if (mode === 'main' && (event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') { event.preventDefault(); app.commandOpen = !app.commandOpen; }
  }
</script>

<svelte:window onkeydown={keydown} oncontextmenu={event => event.preventDefault()}/>
{#if app.fatal}
  <div class="startup-state"><Brand/><h2>{t("无法打开应用")}</h2><p>{app.fatal}</p><Button onclick={() => location.reload()}>{t("重试")}</Button></div>
{:else if !app.data}
  <div class="startup-state"><LoaderCircle size={22} class="spin"/><span>{t("正在打开…")}</span></div>
{:else if mode === 'selection'}
  <CaptureOverlay/>
{:else if mode === 'quick'}
  <QuickView/>
{:else if mode === 'result-tools'}
  <ResultView floating toolsOnly/>
{:else if mode === 'result'}
  <ResultView floating/>
{:else}
  <div class="app-shell" class:sidebar-collapsed={sidebarCollapsed}>
    <aside class="navigation-rail" aria-label={t("主导航")}>
      <nav class="rail-main">
        <ActiveIndicator selector='[aria-current="page"]'/>
        {#each nav as item}
          <button class="rail-link" aria-label={item.label} title={sidebarCollapsed ? item.label : undefined} onclick={() => app.route = item.route} aria-current={app.route === item.route ? 'page' : undefined}><item.icon size={20}/><span class="rail-label" aria-hidden={sidebarCollapsed}>{item.label}</span></button>
        {/each}
      </nav>
      <div class="rail-bottom">
        <button class="sidebar-toggle" aria-label={sidebarCollapsed ? t("展开侧栏") : t("收起侧栏")} title={sidebarCollapsed ? t("展开侧栏") : t("收起侧栏")} aria-expanded={!sidebarCollapsed} onclick={toggleSidebar}><PanelsTopLeft size={19}/></button>
      </div>
    </aside>
    <main class="main-content" aria-label={nav.find(item => item.route === app.route)?.label || t("组件")}>
      <div class="page-container" class:settings-page={app.route === 'settings'}>
        {#key app.route}
        <div class="page-body">
          {#if app.route === 'shortcuts'}<ShortcutsView/>
          {:else if app.route === 'translate'}<TextView/>
          {:else if app.route === 'capture'}<ResultView/>
          {:else if app.route === 'providers'}<ServicesView/>
          {:else if app.route === 'settings'}<SettingsView onwelcome={() => welcome = true}/>
          {:else if import.meta.env.DEV}{#await import('./views/ComponentsView.svelte') then module}<module.default/>{/await}{/if}
        </div>
        {/key}
      </div>
      {#if app.data?.capabilities.platform !== 'windows' && updates.stage === 'available' && updates.version !== updates.dismissed}
        <aside class="update-banner" aria-label={t('软件更新')}><Download size={18}/><span>{t('发现新版本 {0}',updates.version)}</span><Button variant="outline" size="sm" onclick={()=>{app.route='settings';app.settingsTab='about';updates.dismissed=updates.version;}}>{t('查看')}</Button><button class="icon-button" aria-label={t('稍后')} onclick={()=>updates.dismissed=updates.version}><X size={16}/></button></aside>
      {/if}
    </main>
  </div>
  <UpdateDialog />
  {#if app.commandOpen}{#await import('$lib/components/CommandPalette.svelte') then module}<module.default/>{/await}{/if}
  {#if welcome}{#await import('$lib/components/Onboarding.svelte') then module}<module.default bind:open={welcome}/>{/await}{/if}
{/if}
<Toaster position="bottom-center" offset="18px" visibleToasts={2} theme={app.data?.config.appearance.theme ?? 'system'} toastOptions={{duration:2600,classes:{toast:'app-toast',icon:'app-toast-icon',content:'app-toast-content',title:'app-toast-title',description:'app-toast-description',actionButton:'app-toast-action'}}}/>

