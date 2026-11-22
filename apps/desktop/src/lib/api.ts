import { invoke, isTauri } from '@tauri-apps/api/core';
import type { Bootstrap } from './generated';

export const desktop = isTauri();

export async function call<T = void>(command: string, arguments_: Record<string, unknown> = {}): Promise<T> {
  if (!desktop) {
    if (command === 'bootstrap') {
      const { previewBootstrap } = await import('./preview');
      return previewBootstrap() as T;
    }
    if (command === 'copy_text') { await navigator.clipboard.writeText(String(arguments_.text)); return undefined as T; }
    if (command === 'local_ocr_status') {
      return {
        supported: true,
        state: 'not_installed',
        modelVersion: 'ppocrv5-mobile-v1',
        installedBytes: 0,
        downloadedBytes: 0,
        totalBytes: 37_600_000,
        workerRunning: false,
        error: null,
      } as T;
    }
    throw { code: 'unsupported', message: '此功能需要 ScreenTranslate 桌面应用；当前是浏览器界面预览。', action: null };
  }
  return invoke<T>(command, arguments_);
}

export async function loadBootstrap(): Promise<Bootstrap> { return call<Bootstrap>('bootstrap'); }
