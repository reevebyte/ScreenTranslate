import { config as base } from './wdio.conf.mjs';
export const config = {
  ...base,
  specs: ['./tests/e2e/soft-ui.mjs'],
  services: base.services.map(([name, options]) => [name, { ...options, captureBackendLogs: true }]),
};
