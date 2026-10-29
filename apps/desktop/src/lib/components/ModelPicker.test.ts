import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, waitFor } from '@testing-library/svelte';
import { tick } from 'svelte';
import ModelPicker from './ModelPicker.svelte';

afterEach(cleanup);
it('discards an old endpoint response after switching connections', async () => {
  let finish!: (models: string[]) => void;
  const first = vi.fn(() => new Promise<string[]>(resolve => finish = resolve));
  const second = vi.fn().mockResolvedValue([' new-model ', 'new-model']);
  const view = render(ModelPicker, { identity:'first', load:first });
  await fireEvent.click(view.getByRole('button', { name:'选择模型' }));
  await waitFor(() => expect(first).toHaveBeenCalledOnce());
  await view.rerender({ identity:'second', load:second });
  await fireEvent.click(view.getByRole('button', { name:'选择模型' }));
  await waitFor(() => expect(view.getAllByText('new-model')).toHaveLength(1));
  finish(['stale-model']); await tick();
  expect(view.queryByText('stale-model')).toBeNull();
  expect(view.getAllByText('new-model')).toHaveLength(1);
});

it('keeps manual input available after failure and retries without losing it', async () => {
  const load = vi.fn().mockRejectedValueOnce(new Error('请求限流')).mockResolvedValueOnce(['manual-model']);
  const view = render(ModelPicker, { identity:'same', load, value:'manual-model' });
  await fireEvent.click(view.getByRole('button', { name:'选择模型' }));
  await waitFor(() => expect(view.getByRole('alert').textContent).toContain('请求限流'));
  expect((view.container.querySelector('input') as HTMLInputElement).value).toBe('manual-model');
  await fireEvent.click(view.getByRole('button', { name:'重试' }));
  await waitFor(() => expect(view.getByText('manual-model')).toBeTruthy());
  expect(load).toHaveBeenCalledTimes(2);
});
