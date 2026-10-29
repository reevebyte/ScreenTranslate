import { config as base } from './wdio.conf.mjs';
export const config = { ...base, specs: ['./tests/e2e/native-layout.mjs'], mochaOpts: {...base.mochaOpts,timeout:process.env.SCREENTRANS_MANUAL_QA?300000:90000} };
