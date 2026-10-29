import {afterEach,expect,it} from 'vitest';
import {cleanup,render} from '@testing-library/svelte';
import ProviderIcon from './ProviderIcon.svelte';
afterEach(cleanup);
it('keeps gradient references inside each icon when hidden copies and selected details coexist',async()=>{
  const hidden=render(ProviderIcon,{brand:'bing'});hidden.container.style.display='none';
  const visible=render(ProviderIcon,{brand:'bing'});const azure=render(ProviderIcon,{brand:'azure'});
  for(const svg of document.querySelectorAll('svg'))for(const path of svg.querySelectorAll('[fill^="url(#"]')){
    const id=path.getAttribute('fill')!.slice(5,-1);expect(svg.querySelector('[id="'+id+'"]')).not.toBeNull();
  }
  const ids=[...document.querySelectorAll('svg [id]')].map(n=>n.id);expect(new Set(ids).size).toBe(ids.length);
  hidden.unmount();await visible.rerender({brand:'azure'});
  const next=[...document.querySelectorAll('svg [id]')].map(n=>n.id);expect(new Set(next).size).toBe(next.length);
  expect(azure.container.querySelectorAll('linearGradient').length).toBeGreaterThan(0);
});
