import en from './locales/en.json';
import { isTauri } from '@tauri-apps/api/core';
import { emit, listen } from '@tauri-apps/api/event';
export type UiLocale='zh-CN'|'en';
const key='screentranslate-ui-language';
function stored():UiLocale {try{return localStorage.getItem(key)==='en'?'en':'zh-CN';}catch{return 'zh-CN';}}
export const uiLocale=$state<{current:UiLocale}>({current:stored()});
export function translate(text:string,language:UiLocale,...values:unknown[]):string {
  const message=language==='en'?(en as Record<string,string>)[text]??text:text;
  return message.replace(/\{(\d+)\}/g,(match,index)=>index<values.length?String(values[Number(index)]):match);
}
export function t(text:string,...values:unknown[]){return translate(text,uiLocale.current,...values);}
function apply(language:UiLocale){uiLocale.current=language;document.documentElement.lang=language;}
export function setLocale(value:string){const language:UiLocale=value==='en'?'en':'zh-CN';apply(language);try{localStorage.setItem(key,language);}catch{/* Storage may be unavailable. */}if(isTauri())void emit('ui-locale-changed',{language}).catch(()=>{});}
export function initializeLocale(){apply(stored());let disposed=false;let stop:(()=>void)|undefined;const onStorage=(event:StorageEvent)=>{if(event.key===key)apply(stored());};window.addEventListener('storage',onStorage);if(isTauri())void listen<{language:UiLocale}>('ui-locale-changed',({payload})=>{if(!disposed && ['zh-CN','en'].includes(payload.language))apply(payload.language);}).then(fn=>{if(disposed)fn();else stop=fn;});return()=>{disposed=true;stop?.();window.removeEventListener('storage',onStorage);};}
