import { t } from './i18n.svelte';
import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

export type WithoutChild<T> = T extends unknown ? Omit<T, 'child'> : never;
export type WithoutChildren<T> = T extends unknown ? Omit<T, 'children'> : never;
export type WithoutChildrenOrChild<T> = T extends unknown ? Omit<T, 'children' | 'child'> : never;
export type WithElementRef<T, U extends HTMLElement = HTMLElement> = T & { ref?: U | null };

export function cn(...inputs: ClassValue[]) { return twMerge(clsx(inputs)); }

export const modifierLabel = /Mac/.test(navigator.userAgent) ? '⌘' : 'Ctrl';

export const languages = [
  ['auto', '自动识别'], ['zh-Hans', '简体中文'], ['zh-Hant', '繁體中文'], ['en', '英语'],
  ['ja', '日语'], ['ko', '韩语'], ['fr', '法语'], ['de', '德语'], ['es', '西班牙语'], ['ru', '俄语'],
] as const;

export const languageOptions = () => languages.map(([code,label]) => [code,t(label)] as const);

export const languageName = (value: string) => t(languages.find(([code]) => code === value)?.[1] ?? value);

export function errorMessage(error: unknown): string {
  if (error && typeof error === 'object' && 'message' in error) return t(String(error.message));
  return typeof error === 'string' ? t(error) : t('操作未完成，请重试');
}

export function frameUrl(id: string): string {
  if (!('__TAURI_INTERNALS__' in window)) return '';
  return /Windows/.test(navigator.userAgent) ? `http://frame.localhost/${encodeURIComponent(id)}` : `frame://localhost/${encodeURIComponent(id)}`;
}
