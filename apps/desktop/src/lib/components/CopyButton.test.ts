import {beforeEach,afterEach,expect,it,vi} from 'vitest';
import {render,fireEvent,cleanup,waitFor} from '@testing-library/svelte';
const {call,toast}=vi.hoisted(()=>({call:vi.fn(),toast:{success:vi.fn(),error:vi.fn()}}));
vi.mock('$lib/api',()=>({desktop:true,call,loadBootstrap:vi.fn()}));
vi.mock('svelte-sonner',()=>({toast}));
import CopyButton from './CopyButton.svelte';
beforeEach(()=>{vi.clearAllMocks();call.mockResolvedValue(undefined);});afterEach(cleanup);
it('shows only a check in the original button after successful copying',async()=>{
 const view=render(CopyButton,{text:'Source fixture',label:'复制原文'});const button=view.getByRole('button',{name:'复制原文'});
 await fireEvent.click(button);await waitFor(()=>expect(button.querySelector('.copy-done')).not.toBeNull());
 expect(call).toHaveBeenCalledWith('copy_text',{text:'Source fixture'});expect(button.textContent).not.toContain('复制');expect(toast.success).not.toHaveBeenCalled();
});
it('does not show a successful check when clipboard writing fails',async()=>{
 call.mockRejectedValueOnce({message:'Clipboard unavailable'});
 const view=render(CopyButton,{text:'Fixture',label:'复制译文'});const button=view.getByRole('button',{name:'复制译文'});
 await fireEvent.click(button);await waitFor(()=>expect(toast.error).toHaveBeenCalled());expect(button.querySelector('.copy-done')).toBeNull();
});
