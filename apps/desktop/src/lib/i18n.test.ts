import {afterEach,expect,it} from 'vitest';
import {render,fireEvent,cleanup,waitFor} from '@testing-library/svelte';
import {translate,setLocale,uiLocale} from './i18n.svelte';
import {app} from './store.svelte';
import {previewBootstrap} from './preview';
import SettingsView from '../views/SettingsView.svelte';
afterEach(()=>{cleanup();setLocale('zh-CN');});
it('changes existing settings immediately and persists only the interface language',async()=>{
  setLocale('zh-CN');app.data=previewBootstrap();app.settingsTab='general';app.input='原文不会变';const config=JSON.stringify(app.data.config);
  const view=render(SettingsView);await fireEvent.click(view.getByRole('combobox',{name:'界面语言'}));await fireEvent.pointerUp(view.getByRole('option',{name:'English'}),{button:0,pointerType:'mouse'});
  await waitFor(()=>expect(view.getByRole('tab',{name:'General'})).toBeTruthy());expect(view.getByText('Translate as you type')).toBeTruthy();expect(uiLocale.current).toBe('en');expect(localStorage.getItem('screentranslate-ui-language')).toBe('en');expect(app.input).toBe('原文不会变');expect(JSON.stringify(app.data.config)).toBe(config);
});
it('formats translated messages and preserves text that is not interface copy',()=>{expect(translate('发现新版本 {0}','en','2.0.1')).toBe('Version 2.0.1 is available');expect(translate('设置','en')).toBe('Settings');expect(translate('This is user content','en')).toBe('This is user content');});
