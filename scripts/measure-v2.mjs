import { readFile, readdir, stat } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';

const root = fileURLToPath(new URL('../', import.meta.url));
const directory = resolve(root, 'apps/desktop/dist');
const manifest = JSON.parse(await readFile(resolve(directory, '.vite/manifest.json'), 'utf8'));
const entries = Object.entries(manifest).filter(([, item]) => item.isEntry).map(([key]) => key);
const visited = new Set();
const initialFiles = new Set();
function visit(key) {
  if (visited.has(key)) return;
  visited.add(key);
  const item = manifest[key];
  if (!item) throw new Error(`Missing manifest chunk: ${key}`);
  initialFiles.add(item.file);
  (item.css || []).forEach(file => initialFiles.add(file));
  (item.imports || []).forEach(visit);
}
entries.forEach(visit);
let javascript = 0;
let css = 0;
for (const file of initialFiles) {
  const size = gzipSync(await readFile(resolve(directory, file))).length;
  if (file.endsWith('.js')) javascript += size;
  if (file.endsWith('.css')) css += size;
}
const allFiles = await readdir(resolve(directory, 'assets'));
let total = 0;
for (const file of allFiles) total += (await stat(resolve(directory, 'assets', file))).size;
const report = {
  initialJavaScriptGzipBytes: javascript,
  initialCssGzipBytes: css,
  totalAssetsBytes: total,
  budgets: { initialJavaScriptGzipBytes: 250 * 1024, initialCssGzipBytes: 60 * 1024 },
};
console.log(JSON.stringify(report, null, 2));
if (javascript > report.budgets.initialJavaScriptGzipBytes || css > report.budgets.initialCssGzipBytes) process.exitCode = 1;
