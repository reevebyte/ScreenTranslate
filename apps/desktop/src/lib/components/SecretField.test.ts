import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, waitFor } from '@testing-library/svelte';
import { previewBootstrap } from '$lib/preview';
const { call } = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock('$lib/api', () => ({ call }));
vi.mock('svelte-sonner', () => ({ toast: { error: vi.fn(), warning: vi.fn() } }));
import { app } from '$lib/store.svelte';
import SecretField from './SecretField.svelte';
beforeEach(() => { vi.clearAllMocks(); app.data = previewBootstrap(); app.data.credentials.azure_ocr = true; call.mockImplementation(async command => command === 'read_secret' ? 'fixture-existing-key' : true); });
afterEach(cleanup);
describe('saved credentials', () => {
  it('flushes a typed credential when its view is removed', async () => {
    const view = render(SecretField, { name: 'azure_ocr', id: 'key', label: 'Vision 密钥' });
    await fireEvent.input(view.getByLabelText('Vision 密钥'), { target: { value: 'fixture-on-leave' } });
    view.unmount();
    await waitFor(() => expect(call).toHaveBeenCalledWith('save_secret', { name: 'azure_ocr', secret: 'fixture-on-leave', persistent: true }));
  });
  it('reveals a stored key on demand and masks it again without rewriting it', async () => {
    const view = render(SecretField, { name: 'azure_ocr', id: 'key', label: 'Vision 密钥' });
    expect(call).not.toHaveBeenCalled();
    await fireEvent.click(view.getByRole('button', { name: '查看Vision 密钥' }));
    await waitFor(() => expect((view.getByLabelText('Vision 密钥') as HTMLInputElement).value).toBe('fixture-existing-key'));
    expect(view.getByLabelText('Vision 密钥').getAttribute('type')).toBe('text');
    await fireEvent.click(view.getByRole('button', { name: '隐藏Vision 密钥' }));
    expect(view.getByLabelText('Vision 密钥').getAttribute('type')).toBe('password');
    await fireEvent.blur(view.getByLabelText('Vision 密钥'));
    expect(call.mock.calls.filter(([command]) => command === 'save_secret')).toHaveLength(0);
  });
  it('saves a replacement on blur and retains it for later reveal', async () => {
    const view = render(SecretField, { name: 'azure_ocr', id: 'key', label: 'Vision 密钥' });
    await fireEvent.input(view.getByLabelText('Vision 密钥'), { target: { value: 'fixture-replacement' } });
    await fireEvent.blur(view.getByLabelText('Vision 密钥'));
    await waitFor(() => expect(call).toHaveBeenCalledWith('save_secret', { name: 'azure_ocr', secret: 'fixture-replacement', persistent: true }));
    await fireEvent.click(view.getByRole('button', { name: '查看Vision 密钥' }));
    expect((view.getByLabelText('Vision 密钥') as HTMLInputElement).value).toBe('fixture-replacement');
  });
});
