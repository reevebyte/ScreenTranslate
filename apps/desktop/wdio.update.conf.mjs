import {config as base} from './wdio.conf.mjs';
export const config={...base,specs:['./tests/e2e/update-ui.mjs']};
