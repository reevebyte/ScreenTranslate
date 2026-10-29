import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, waitFor } from '@testing-library/svelte';
import { previewBootstrap } from '$lib/preview';
import { tick } from 'svelte';

vi.mock('$lib/api', () => ({ desktop: false, call: vi.fn(), loadBootstrap: vi.fn() }));
vi.mock('svelte-sonner', () => ({ toast: { error: vi.fn(), success: vi.fn(), info: vi.fn() } }));
import { app } from '$lib/store.svelte';
import ServicesView from './ServicesView.svelte';
import { call } from '$lib/api';

beforeEach(() => {
  vi.mocked(call).mockReset();
  vi.stubGlobal('matchMedia', () => ({ matches: false }));
  app.data = previewBootstrap();
  app.servicesTab = 'translation';
  app.configError = '';
});
afterEach(async () => { cleanup(); await tick(); });

it('waits for endpoint editing to finish before saving the connection', async () => {
  const view = render(ServicesView);
  await fireEvent.click(view.getByRole('button', { name: 'OpenAI 兼容' }));
  const field = view.getByLabelText('服务地址');
  const previous = app.data!.config.providers.openai!.baseUrl;
  await fireEvent.focusIn(field);
  await fireEvent.input(field, { target: { value: 'http://localhost:4010/v1' } });
  expect(app.data!.config.providers.openai!.baseUrl).toBe(previous);
  await fireEvent.focusOut(field);
  await waitFor(() => expect(app.data!.config.providers.openai!.baseUrl).toBe('http://localhost:4010/v1'));
});

it('publishes edited fields synchronously on focusout before the next page snapshots config', async () => {
  const view=render(ServicesView);
  await fireEvent.click(view.getByRole('button',{name:'OpenAI 兼容'}));
  const field=view.getByLabelText('模型');
  await fireEvent.focusIn(field);
  await fireEvent.input(field,{target:{value:'keep-on-navigation'}});
  field.dispatchEvent(new FocusEvent('focusout',{bubbles:true}));
  // A newly mounted settings form snapshots this value before Svelte flushes effects.
  expect(app.data!.config.providers.openai!.model).toBe('keep-on-navigation');
});

it('preserves the chosen translator while changing OCR in another tab', async () => {
  const view = render(ServicesView);
  await fireEvent.click(view.getByRole('button', { name: 'OpenAI 兼容' }));
  await fireEvent.click(view.getByRole('button', { name: '设为当前' }));
  await waitFor(() => expect(app.data!.config.provider).toBe('openai'));
  await fireEvent.click(view.getByRole('tab', { name: '文字识别' }));
  await fireEvent.click(await view.findByRole('button', { name: 'Azure Vision' }));
  expect(app.data!.config.ocr.engine).toBe('youdao');
  await fireEvent.click(view.getByRole('button', { name: '设为当前' }));
  await waitFor(() => expect(app.data!.config.ocr.engine).toBe('azure'));
  expect(app.data!.config.provider).toBe('openai');
  expect(view.container.querySelectorAll('.service-detail')).toHaveLength(1);
  await fireEvent.click(view.getByRole('tab', { name: '翻译接口' }));
  expect(view.getByRole('button', { name: 'OpenAI 兼容' }).getAttribute('aria-pressed')).toBe('true');
  expect(app.data!.config.ocr.engine).toBe('azure');
});

it('groups manufacturers and free APIs without changing the active connection while browsing', async () => {
  app.data!.config.provider='nvidia';app.data!.config.providers.nvidia!.baseUrl='https://custom.nvidia.example/v1';app.data!.credentials.nvidia=true;
  const view=render(ServicesView);
  expect(view.container.querySelector('.service-in-use')).not.toBeNull();
  await fireEvent.click(view.getByRole('button',{name:'Google 网页'}));
  expect(view.container.querySelectorAll('.service-list-group')).toHaveLength(2);
  expect(app.data!.config.provider).toBe('nvidia');
  await fireEvent.click(view.getByRole('button',{name:'OpenAI 兼容'}));
  await fireEvent.click(view.getByText('接口设置'));
  await fireEvent.click(view.getByRole('combobox',{name:'服务预设'}));
  await fireEvent.click(view.getByRole('option',{name:'OpenAI'}));
  await waitFor(()=>expect(view.getByLabelText('服务地址').getAttribute('id')).toBe('provider-endpoint'));
  expect(app.data!.config.providers.nvidia!.baseUrl).toBe('https://custom.nvidia.example/v1');
  expect(app.data!.credentials.nvidia).toBe(true);
  expect(app.data!.config.provider).toBe('nvidia');
});
it('configures custom OCR without enabling it just by inspecting its form', async () => {
  app.servicesTab='ocr';const view=render(ServicesView);
  await fireEvent.click(view.getByRole('button',{name:'自定义 OCR'}));
  expect(app.data!.config.ocr.engine).toBe('youdao');
  const button=view.getByRole('button',{name:'设为当前'}) as HTMLButtonElement;expect(button.disabled).toBe(true);
  await fireEvent.input(view.getByLabelText('服务地址'),{target:{value:'http://localhost:4010/v1'}});
  await fireEvent.input(view.getByLabelText('视觉模型'),{target:{value:'vision-local'}});
  expect(button.disabled).toBe(false);
  await fireEvent.click(button);
  await waitFor(()=>expect(app.data!.config.ocr.engine).toBe('custom'));
  expect(app.data!.config.ocr.custom.model).toBe('vision-local');
});

it('keeps DeepL plan and endpoint in sync when toggling advanced configuration', async()=>{
  const view=render(ServicesView);await fireEvent.click(view.getByRole('button',{name:'DeepL'}));
  const choice=view.getByLabelText('使用 DeepL API Free');await fireEvent.click(choice);
  await waitFor(()=>expect(app.data!.config.providers.deepl!.baseUrl).toBe('https://api.deepl.com/v2'));
  expect(app.data!.config.providers.deepl!.freePlan).toBe(false);expect(app.data!.config.provider).toBe('google_free');
  await fireEvent.click(choice);await waitFor(()=>expect(app.data!.config.providers.deepl!.baseUrl).toBe('https://api-free.deepl.com/v2'));expect(app.data!.config.providers.deepl!.freePlan).toBe(true);
});

it('shows connection failure inline and clears it when relevant fields change', async () => {
  vi.mocked(call).mockRejectedValue({message:'请求限流，请稍后重试'});
  const view=render(ServicesView);
  await fireEvent.click(view.getByRole('button',{name:'OpenAI 兼容'}));
  await fireEvent.click(view.getByRole('button',{name:'测试连接'}));
  await waitFor(()=>expect(view.getByRole('button',{name:/连接失败：请求限流/})).toBeTruthy());
  expect(view.container.querySelector('.service-feedback')).not.toBeNull();
  await fireEvent.input(view.getByLabelText('模型'),{target:{value:'another-model'}});
  expect(view.queryByRole('button',{name:/连接失败/})).toBeNull();
});

it('does not show a previous service test result after switching to another service', async () => {
  let finish!:(value:string)=>void;
  vi.mocked(call).mockImplementation(()=>new Promise(resolve=>finish=resolve));
  const view=render(ServicesView);
  await fireEvent.click(view.getByRole('button',{name:'测试连接'}));
  await waitFor(()=>expect(call).toHaveBeenCalledWith('test_provider',{provider:'google_free'}));
  await fireEvent.click(view.getByRole('button',{name:'DeepL'}));
  finish('stale success');await tick();
  expect(view.queryByRole('button',{name:/连接成功/})).toBeNull();
  expect((view.getByRole('button',{name:'测试连接'}) as HTMLButtonElement).disabled).toBe(false);
});
