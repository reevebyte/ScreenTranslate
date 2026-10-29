import { render, fireEvent, waitFor, cleanup } from '@testing-library/svelte';
import { beforeEach, afterEach, it, expect, vi } from 'vitest';
import { previewBootstrap } from '$lib/preview';
const { saveConfig, app } = vi.hoisted(() => ({saveConfig:vi.fn(), app:{data:null as ReturnType<typeof previewBootstrap> | null,route:'translate'}}));
vi.mock('$lib/store.svelte',()=>({app,saveConfig}));
import Onboarding from './Onboarding.svelte';
beforeEach(()=>{app.data=previewBootstrap();app.data.config.onboardingComplete=false;saveConfig.mockReset().mockResolvedValue(true);app.route="translate";});
afterEach(cleanup);
it('remembers closing the first-run guide without replacing the selected provider', async()=>{
 app.data!.config.provider='openai';
 const view=render(Onboarding,{open:true});
 await fireEvent.click(view.getByRole('button',{name:'稍后配置'}));
 await waitFor(()=>expect(saveConfig).toHaveBeenCalledWith(expect.objectContaining({onboardingComplete:true,provider:'openai'}),{quiet:true}));
});

it('opens shortcuts after starting and keeps the guide open when saving fails',async()=>{
 const view=render(Onboarding,{open:true});
 saveConfig.mockResolvedValueOnce(false);
 await fireEvent.click(view.getByRole('button',{name:'开始使用'}));
 expect(view.getByRole('dialog')).toBeTruthy();
 expect(app.route).toBe('translate');
 await fireEvent.click(view.getByRole('button',{name:'开始使用'}));
 await waitFor(()=>expect(app.route).toBe('shortcuts'));
});
