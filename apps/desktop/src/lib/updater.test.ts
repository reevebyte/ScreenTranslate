import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { previewBootstrap } from './preview';
const { call, listeners } = vi.hoisted(()=>({call:vi.fn(),listeners:new Map<string,(event:{payload:unknown})=>void>()}));
vi.mock('./api',()=>({desktop:true,call,loadBootstrap:vi.fn()}));
vi.mock('@tauri-apps/api/event',()=>({listen:vi.fn(async(name:string,fn:(e:{payload:unknown})=>void)=>{listeners.set(name,fn);return ()=>listeners.delete(name);})}));
import {app} from './store.svelte';
import {updates,checkUpdate,downloadUpdate,installUpdate,initializeUpdates,consumeUpdateRequest,syncUpdateUi,scheduleAutomaticCheck,AUTO_CHECK_INTERVAL_MS,AUTO_CHECK_RETRY_DELAY_MS} from './updater.svelte';
beforeEach(()=>{call.mockReset().mockResolvedValue(undefined);app.data=previewBootstrap();app.configPending=false;app.configSaving=false;Object.assign(updates,{stage:'idle',version:'',notes:'',progress:null,error:'',dismissed:'',confirm:false,connected:false,upToDateNotice:false});});
afterEach(()=>{scheduleAutomaticCheck(false);vi.useRealTimers();listeners.clear();});
it('serializes checks and retains the downloaded version across checks',async()=>{
 let resolve!:(v:unknown)=>void;call.mockImplementationOnce(()=>new Promise(r=>resolve=r));
 const first=checkUpdate();await checkUpdate();expect(call).toHaveBeenCalledTimes(1);
 resolve({available:true,version:'2.0.2',notes:'Fixture notes'});await first;
 expect(updates.stage).toBe('available');
 call.mockResolvedValueOnce(undefined);await downloadUpdate();expect(updates.stage).toBe('ready');
 await checkUpdate();expect(call).toHaveBeenCalledTimes(2);
});
it('keeps download errors recoverable and does not install without verified bytes',async()=>{
 updates.version='2.0.2';updates.stage='available';
 call.mockRejectedValueOnce({message:'更新下载或签名校验失败，请重试'});await downloadUpdate();
 expect(updates.stage).toBe('error');await installUpdate();expect(call).toHaveBeenCalledTimes(1);
 call.mockResolvedValueOnce(undefined);await downloadUpdate();expect(updates.stage).toBe('ready');
});
it('blocks installation during translation or unsaved settings and retains ready bytes after failure',async()=>{
 updates.stage='ready';updates.version='2.0.2';app.configPending=true;await installUpdate();expect(call).not.toHaveBeenCalled();
 app.configPending=false;app.data!.text.job.stage='translating';await installUpdate();expect(call).not.toHaveBeenCalled();
 app.data!.text.job.stage='complete';call.mockRejectedValueOnce({message:'无法启动更新安装，请重试'});await installUpdate();expect(updates.stage).toBe('ready');expect(updates.error).toBeTruthy();
});
it('uses cumulative progress and ignores late progress after download',async()=>{
 const stop=initializeUpdates();await vi.waitFor(()=>expect(updates.connected).toBe(true));updates.stage='downloading';
 listeners.get('update-progress')?.({payload:{downloaded:50,total:100}});expect(updates.progress).toBe(50);
 listeners.get('update-progress')?.({payload:{downloaded:150,total:100}});expect(updates.progress).toBe(99);
 updates.stage='ready';updates.progress=100;listeners.get('update-progress')?.({payload:{downloaded:1,total:100}});expect(updates.progress).toBe(100);stop();expect(listeners.size).toBe(0);
});

it('downloads and installs on caption click when the package is ready',async()=>{
 updates.version='2.0.3';updates.stage='available';updates.confirm=false;
 call.mockResolvedValueOnce('caption').mockResolvedValueOnce(undefined);await consumeUpdateRequest();
 expect(updates.stage).toBe('ready');expect(updates.confirm).toBe(false);
 call.mockResolvedValueOnce('caption').mockResolvedValueOnce(undefined);await consumeUpdateRequest();
 expect(call).toHaveBeenCalledWith('install_update',{version:'2.0.3'});
});
it('updates directly from the tray while retaining the busy-work gate',async()=>{
 updates.version='2.0.3';updates.stage='available';app.configPending=true;
 call.mockResolvedValueOnce('tray');await consumeUpdateRequest();expect(call).toHaveBeenCalledTimes(1);
 app.configPending=false;call.mockResolvedValueOnce('tray').mockResolvedValueOnce(undefined).mockResolvedValueOnce(undefined);
 await consumeUpdateRequest();expect(call).toHaveBeenCalledWith('install_update',{version:'2.0.3'});
});
it('synchronizes tray and caption states including language and busy work',()=>{
 updates.connected=true;updates.version='2.0.3';updates.stage='available';updates.dismissed='';syncUpdateUi();
 expect(call).toHaveBeenCalledWith('set_tray_update',expect.objectContaining({version:'2.0.3',blocked:false}));
 expect(call).toHaveBeenCalledWith('set_update_caption',expect.objectContaining({visible:true,mode:'download',enabled:true}));
 app.configPending=true;syncUpdateUi();expect(call).toHaveBeenLastCalledWith('set_update_caption',expect.objectContaining({enabled:false}));
});
it('shows the centered up-to-date notice only on manual check',async()=>{
 call.mockResolvedValueOnce({available:false});await checkUpdate();
 expect(updates.stage).toBe('current');expect(updates.upToDateNotice).toBe(false);
 call.mockResolvedValueOnce({available:false});await checkUpdate(true);
 expect(updates.stage).toBe('current');expect(updates.upToDateNotice).toBe(true);
});
it('treats "no update published" as up to date in the English UI too',async()=>{
 const {setLocale}=await import('./i18n.svelte');
 setLocale('en');
 try{
  call.mockRejectedValueOnce({code:'network',message:'尚未发布可用更新'});await checkUpdate(true);
  expect(updates.stage).toBe('current');expect(updates.upToDateNotice).toBe(true);expect(updates.error).toBe('');
 }finally{setLocale('zh-CN');}
});
it('waits for a running check instead of dropping a tray update request',async()=>{
 let resolve!:(v:unknown)=>void;
 call.mockImplementationOnce(()=>new Promise(r=>resolve=r));
 const automatic=checkUpdate();
 call.mockResolvedValueOnce('tray').mockResolvedValueOnce(undefined).mockResolvedValueOnce(undefined);
 const request=consumeUpdateRequest();
 await vi.waitFor(()=>expect(call).toHaveBeenCalledWith('take_update_request'));
 resolve({available:true,version:'2.0.9'});await automatic;await request;
 expect(call).toHaveBeenCalledWith('download_update',{version:'2.0.9'});
 expect(call).toHaveBeenCalledWith('install_update',{version:'2.0.9'});
});
it('does not leak a rejection when the update request cannot be read',async()=>{
 call.mockRejectedValueOnce(new Error('ipc down'));
 await expect(consumeUpdateRequest()).resolves.toBeUndefined();
});
it('checks periodically every 4 hours, retries transient failures silently in 15 minutes, and resumes when re-enabled',async()=>{
 vi.useFakeTimers();
 call
  .mockRejectedValueOnce({code:'network',message:'无法检查更新，请稍后重试'})
  .mockResolvedValueOnce({available:false})
  .mockResolvedValueOnce({available:true,version:'2.0.42',notes:'notes'});
 scheduleAutomaticCheck(true, 100);
 await vi.advanceTimersByTimeAsync(100);
 expect(call).toHaveBeenCalledTimes(1);
 // Silent background failure must not pollute the About page with an error banner.
 expect(updates.stage).toBe('idle');
 expect(updates.error).toBe('');
 await vi.advanceTimersByTimeAsync(AUTO_CHECK_RETRY_DELAY_MS);
 expect(call).toHaveBeenCalledTimes(2);
 expect(updates.stage).toBe('current');
 await vi.advanceTimersByTimeAsync(AUTO_CHECK_INTERVAL_MS);
 expect(call).toHaveBeenCalledTimes(3);
 expect(updates.stage).toBe('available');
 expect(updates.version).toBe('2.0.42');
 // Toggling auto-check off then on re-schedules cleanly.
 scheduleAutomaticCheck(false);
 updates.stage = 'idle';
 updates.version = '';
 call.mockResolvedValueOnce({available:false});
 scheduleAutomaticCheck(true, 50);
 await vi.advanceTimersByTimeAsync(50);
 expect(call).toHaveBeenCalledTimes(4);
});
it('switches to About when clicking the tray update entry',async()=>{
 app.route = 'translate';
 app.settingsTab = 'general';
 updates.version='2.0.42';updates.stage='available';
 call.mockResolvedValueOnce('tray').mockResolvedValueOnce(undefined).mockResolvedValueOnce(undefined);
 await consumeUpdateRequest();
 expect(app.route).toBe('settings');
 expect(app.settingsTab).toBe('about');
 expect(call).toHaveBeenCalledWith('download_update',{version:'2.0.42'});
 expect(call).toHaveBeenCalledWith('install_update',{version:'2.0.42'});
});
