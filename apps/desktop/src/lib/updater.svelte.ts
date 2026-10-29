import { listen } from '@tauri-apps/api/event';
import { call, desktop } from './api';
import { app } from './store.svelte';
import { errorMessage } from './utils';
import { t, uiLocale } from './i18n.svelte';

type Stage = 'idle' | 'checking' | 'current' | 'available' | 'downloading' | 'ready' | 'installing' | 'error';
type Info = { available: boolean; version?: string; notes?: string; downloaded?: boolean };
export const updates = $state({stage:'idle' as Stage,version:'',notes:'',progress:null as number|null,error:'',dismissed:'',confirm:false,connected:false});
let operation = false;
let checked = false;
let timer: ReturnType<typeof setTimeout> | undefined;
export function updateBlockReason() {
  if (app.configPending || app.configSaving) return t('请先保存当前配置，再安装更新');
  if (app.data && [app.data.text.job.stage,app.data.visual.job.stage].some(stage=>['capturing','recognizing','translating'].includes(stage))) return t('请等待截图或翻译完成，再安装更新');
  return '';
}
export async function checkUpdate() {
  if (operation || updates.stage === 'ready') return;
  operation = true;
  updates.stage = 'checking'; updates.error = '';
  try {
    const result = await call<Info>('check_update');
    updates.version = result.version ?? ''; updates.notes = result.notes ?? '';
    updates.stage = result.available ? result.downloaded ? 'ready' : 'available' : 'current';
  } catch(error) { updates.error = errorMessage(error); updates.stage = 'error'; }
  finally { operation = false; }
}
export async function downloadUpdate() {
  if (operation || !updates.version) return;
  operation = true; updates.stage = 'downloading'; updates.progress = null; updates.error = '';
  try { await call('download_update',{version:updates.version}); updates.stage = 'ready'; updates.progress = 100; }
  catch(error) { updates.error = errorMessage(error); updates.stage = 'error'; }
  finally { operation = false; }
}
export async function installUpdate() {
  if (operation || updates.stage !== 'ready' || updateBlockReason()) return;
  operation = true; updates.stage = 'installing'; updates.error = '';
  try { await call('install_update',{version:updates.version}); }
  catch(error) { updates.error = errorMessage(error); updates.stage = 'ready'; }
  finally { operation = false; }
}
export function scheduleAutomaticCheck(enabled: boolean, delay = 5000) {
  clearTimeout(timer);
  if (!desktop || import.meta.env.DEV || import.meta.env.VITE_DESKTOP_E2E === 'true' || !enabled || checked) return;
  timer = setTimeout(()=>{ checked = true; void checkUpdate(); },delay);
}

let consuming = false;
export async function consumeUpdateRequest() {
  if (!desktop || !app.data || consuming) return;
  consuming = true;
  try {
    const source = await call<string|null>('take_update_request');
    if (!source) return;
    if (!updates.version) await checkUpdate();
    if (updateBlockReason()) { updates.error=updateBlockReason(); app.route='settings';app.settingsTab='about';return; }
    if (source === 'tray') {
      if (updates.stage === 'available' || updates.stage === 'error') await downloadUpdate();
      if (updates.stage === 'ready') await installUpdate();
    } else if (updates.stage === 'ready') updates.confirm = true;
    else if (updates.stage === 'available' || updates.stage === 'error') await downloadUpdate();
  } finally { consuming = false; }
}
export function syncUpdateUi() {
  if (!desktop || !app.data || !updates.connected) return;
  const version=updates.version||null, blocked=!!updateBlockReason(), english=uiLocale.current==='en';
  void call('set_tray_update',{stage:updates.stage,version,blocked,english}).catch(()=>{});
  void call('set_update_caption',{
    visible:!!version && version!==updates.dismissed && ['available','downloading','ready','installing','error'].includes(updates.stage),
    enabled:!blocked && ['available','ready','error'].includes(updates.stage),
    mode:updates.stage==='downloading'?'downloading':['ready','installing'].includes(updates.stage)?'restart':'download',
    progress:updates.progress,version,english,
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
  return ()=>{ disposed = true; updates.connected=false; clearTimeout(timer); cleanup.splice(0).forEach(fn=>fn()); };
}
