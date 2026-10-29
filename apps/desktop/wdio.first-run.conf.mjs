import { config as base } from './wdio.conf.mjs';

export const config={
  ...base,
  specs:['./tests/e2e/first-run.mjs'],
  // Do not apply the normal test setup, which deliberately marks onboarding complete.
  before:async()=>{await browser.$('.app-shell').waitForDisplayed();},
};
