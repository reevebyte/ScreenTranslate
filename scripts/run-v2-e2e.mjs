import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

if (!process.env.npm_execpath) throw new Error('Run this with pnpm --filter @screentranslate/desktop test:e2e');
const root = fileURLToPath(new URL('../', import.meta.url));
const environment = { ...process.env, VITE_DESKTOP_E2E: 'true' };
function run(args) {
  const result = spawnSync(process.execPath, [process.env.npm_execpath, ...args], { cwd: root, env: environment, stdio: 'inherit' });
  if (result.status !== 0) process.exit(result.status || 1);
}
run(['--filter', '@screentranslate/desktop', 'tauri', 'build', '--debug', '--no-bundle', '--features', 'e2e', '--config', 'src-tauri/tauri.e2e.conf.json']);
// The embedded driver keeps native window state within one invocation. Each
// suite gets a fresh driver and fixture server so stale endpoints cannot leak.
for (const config of ['wdio.first-run.conf.mjs','wdio.conf.mjs','wdio.services.conf.mjs','wdio.layout.conf.mjs','wdio.resize.conf.mjs','wdio.update.conf.mjs']) run(['--filter', '@screentranslate/desktop', 'exec', 'wdio', 'run', config]);
