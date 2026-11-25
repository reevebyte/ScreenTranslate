import { listen } from '@tauri-apps/api/event';
import { call, desktop } from './api';
import { app } from './store.svelte';
import { errorMessage } from './utils';
import { t, uiLocale } from './i18n.svelte';

export const AUTO_CHECK_INITIAL_DELAY_MS = 10_000;
export const AUTO_CHECK_INTERVAL_MS = 4 * 60 * 60 * 1000;
export const AUTO_CHECK_RETRY_DELAY_MS = 15 * 60 * 1000;

type Stage = 'idle' | 'checking' | 'current' | 'available' | 'downloading' | 'ready' | 'installing' | 'error';
type Info = { available: boolean; version?: string; notes?: string; downloaded?: boolean };
export const updates = $state({stage:'idle' as Stage,version:'',notes:'',progress:null as number|null,error:'',dismissed:'',confirm:false,connected:false,upToDateNotice:false});
let operation = false;
let autoEnabled = false;
let autoRetried = false;
let timer: ReturnType<typeof setTimeout> | undefined;
let noticeTimer: ReturnType<typeof setTimeout> | undefined;
function showUpToDateNotice() {
  clearTimeout(noticeTimer);
  updates.upToDateNotice = true;
  noticeTimer = setTimeout(() => { updates.upToDateNotice = false; }, 1800);
}
export function updateBlockReason() {
  if (app.configPending || app.configSaving) return t('请先保存当前配置，再安装更新');
  if (app.data && [app.data.text.job.stage,app.data.visual.job.stage].some(stage=>['capturing','recognizing','translating'].includes(stage))) return t('请等待截图或翻译完成，再安装更新');
  return '';
}
let inflightCheck: Promise<boolean> | undefined;
export function checkUpdate(manual = false): Promise<boolean> {
  if (operation || updates.stage === 'ready') return Promise.resolve(true);
  inflightCheck = runCheck(manual).finally(() => { inflightCheck = undefined; });
  return inflightCheck;
}
async function runCheck(manual: boolean): Promise<boolean> {
  operation = true;
  updates.stage = 'checking'; updates.error = ''; updates.upToDateNotice = false;
  try {
    const result = await call<Info>('check_update');
    updates.version = result.version ?? ''; updates.notes = result.notes ?? '';
    updates.stage = result.available ? result.downloaded ? 'ready' : 'available' : 'current';
    if (manual && updates.stage === 'current') showUpToDateNotice();
    return true;
  } catch(error) {
    const msg = errorMessage(error);
    // Match the backend's own text: errorMessage() has already localized `msg`.
    const raw = error && typeof error === 'object' && 'message' in error ? String(error.message) : String(error);
    if (raw.includes('尚未发布可用更新')) {
      updates.stage = 'current';
      if (manual) showUpToDateNotice();
      return true;
    } else if (manual) {
      updates.error = msg;
      updates.stage = 'error';
    } else {
      // Background checks stay quiet on transient network/proxy issues and retry later.
      updates.error = '';
      updates.stage = 'idle';
    }
    return false;
  }
  finally { operation = false; }
}
export async function downloadUpdate() {
  if (operation || !updates.version) return;
  operation = true; updates.stage = 'downloading'; updates.progress = null; updates.error = '';
  try { await call('download_update',{version:updates.version}); updates.stage = 'ready'; updates.progress = 100; }
  catch(error) { updates.error = errorMessage(error); updates.stage = 'error'; }
  finally { operation = false; }
}
let installFallbackTimer: ReturnType<typeof setTimeout> | undefined;
export async function installUpdate() {
  if (operation || updates.stage !== 'ready' || updateBlockReason()) return;
  operation = true; updates.stage = 'installing'; updates.error = '';
  clearTimeout(installFallbackTimer);
  try {
    await call('install_update', { version: updates.version });
    // On Windows the installer runs out-of-process and `install_update` returns
    // `Ok(())` without exiting. If the app is still open (for example UAC or
    // the installer was cancelled), unlock the modal instead of trapping focus.
    installFallbackTimer = setTimeout(() => {
      if (updates.stage === 'installing') updates.stage = 'ready';
    }, 3000);
  }
  catch(error) { updates.error = errorMessage(error); updates.stage = 'ready'; }
  finally { operation = false; }
}
function queueNextAutoCheck(delay: number) {
  clearTimeout(timer);
  if (!autoEnabled) return;
  timer = setTimeout(() => {
    void runAutomaticCheck();
  }, delay);
}
async function runAutomaticCheck() {
  if (!autoEnabled) return;
  if (['available', 'downloading', 'ready', 'installing'].includes(updates.stage)) {
    queueNextAutoCheck(AUTO_CHECK_INTERVAL_MS);
    return;
  }
  const ok = await checkUpdate(false);
  if (!autoEnabled) return;
  if (ok) {
    autoRetried = false;
    queueNextAutoCheck(AUTO_CHECK_INTERVAL_MS);
  } else if (!autoRetried) {
    autoRetried = true;
    queueNextAutoCheck(AUTO_CHECK_RETRY_DELAY_MS);
  } else {
    autoRetried = false;
    queueNextAutoCheck(AUTO_CHECK_INTERVAL_MS);
  }
}
export function scheduleAutomaticCheck(enabled: boolean, delay = AUTO_CHECK_INITIAL_DELAY_MS) {
  if (enabled && autoEnabled && timer !== undefined && delay === AUTO_CHECK_INITIAL_DELAY_MS) {
    return;
  }
  clearTimeout(timer);
  timer = undefined;
  autoEnabled = enabled;
  if (!enabled) {
    clearTimeout(installFallbackTimer);
    installFallbackTimer = undefined;
    autoRetried = false;
    return;
  }
  if (!desktop || (import.meta.env.DEV && import.meta.env.MODE !== 'test') || import.meta.env.VITE_DESKTOP_E2E === 'true') return;
  queueNextAutoCheck(delay);
}

let consuming = false;
export async function consumeUpdateRequest() {
  if (!desktop || !app.data || consuming) return;
  consuming = true;
  try {
    const source = await call<string|null>('take_update_request');
    if (!source) return;
    // Wait for a check that is already running; otherwise this click would be lost.
    if (!updates.version) await (inflightCheck ?? checkUpdate());
    if (updateBlockReason()) { updates.error=updateBlockReason(); app.route='settings';app.settingsTab='about';return; }
    if (source === 'tray') {
      app.route = 'settings';
      app.settingsTab = 'about';
      if (updates.stage === 'available' || updates.stage === 'error') await downloadUpdate();
      if (updates.stage === 'ready') await installUpdate();
    } else if (source === 'caption') {
      if (updates.stage === 'available' || updates.stage === 'error') await downloadUpdate();
      else if (updates.stage === 'ready') await installUpdate();
    } else if (updates.stage === 'ready') updates.confirm = true;
    else if (updates.stage === 'available' || updates.stage === 'error') await downloadUpdate();
  } catch {
    // The request could not be read; the next click asks again.
  } finally { consuming = false; }
}
export function syncUpdateUi() {
  if (!desktop || !app.data || !updates.connected) return;
  const version=updates.version||null, blocked=!!updateBlockReason(), locale=uiLocale.current, english=locale==='en';
  void call('set_tray_update',{stage:updates.stage,version,blocked,english,locale}).catch(()=>{});
  void call('set_update_caption',{
    visible:!!version && version!==updates.dismissed && ['available','downloading','ready','installing','error'].includes(updates.stage),
    enabled:!blocked && ['available','ready','error'].includes(updates.stage),
    mode:updates.stage==='downloading'?'downloading':['ready','installing'].includes(updates.stage)?'restart':'download',
    progress:updates.progress,version,english,locale,
  }).catch(()=>{});
}
export function initializeUpdates() {
  let disposed = false;
  const cleanup: (()=>void)[] = [];
  async function subscribe<T>(name:string, handler:(event:{payload:T})=>void) {
    const stop=await listen<T>(name,handler); if(disposed)stop();else cleanup.push(stop);
  }
  if (desktop) void Promise.all([
    subscribe<{downloaded:number;total:number|null}>('update-progress',({payload})=>{
      if (disposed || updates.stage !== 'downloading') return;
      updates.progress = payload.total && payload.total > 0 ? Math.min(99,Math.floor(payload.downloaded/payload.total*100)) : null;
    }),
    subscribe('app-update-requested',()=>{void consumeUpdateRequest();}),
    subscribe<string>('app-update-caption-dismissed',({payload})=>{if(payload===updates.version)updates.dismissed=payload;}),
  ]).then(()=>{if(!disposed)updates.connected=true;}).catch(()=>{});
  return ()=>{ disposed = true; updates.connected=false; autoEnabled=false; autoRetried=false; clearTimeout(timer); clearTimeout(noticeTimer); clearTimeout(installFallbackTimer); installFallbackTimer=undefined; cleanup.splice(0).forEach(fn=>fn()); };
}
