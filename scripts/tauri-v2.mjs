import { createRequire } from 'node:module';
import { spawn } from 'node:child_process';
import { readFile, copyFile, mkdir, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { cleanBuilds } from './clean-v2-builds.mjs';
const root = fileURLToPath(new URL('../', import.meta.url));
const require = createRequire(join(root, 'apps/desktop/package.json'));
const cli = join(dirname(require.resolve('@tauri-apps/cli')), 'tauri.js');
const args = process.argv.slice(2);
if (args[0] === 'build') await import('./check-v2-version.mjs');
const code = await new Promise((resolve, reject) => {
  const child = spawn(process.execPath, [cli, ...args], { stdio: 'inherit', env: process.env });
  child.on('error', reject); child.on('exit', code => resolve(code ?? 1));
});
if (code !== 0) process.exit(code);
if (args[0] === 'build' && !args.includes('--debug') && process.env.VITE_DESKTOP_E2E !== 'true') {
  const { version } = JSON.parse(await readFile(join(root, 'apps/desktop/package.json'), 'utf8'));
  if (process.platform === 'win32') await copyFile(join(root, 'target/release/screentranslate.exe'), join(root, `target/release/ScreenTranslate-${version}.exe`));
  await mkdir(join(root, `build/release-${version}`), { recursive: true });
  const report = await cleanBuilds({ root, version, dryRun: false });
  await writeFile(join(root, `build/release-${version}/cleanup.json`), JSON.stringify(report, null, 2));
  console.log(`Cleaned ${(report.removedBytes / 1024 ** 3).toFixed(2)} GiB of earlier build output; ${report.skipped.length} locked or protected paths skipped.`);
}
