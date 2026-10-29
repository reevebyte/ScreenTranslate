import { readdir, realpath, lstat, stat, rm, writeFile, mkdir } from 'node:fs/promises';
import { resolve, relative, isAbsolute, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { releaseDirectory, buildTarget } from './tauri-build-output.mjs';

async function entries(path) { try { return await readdir(path, { withFileTypes: true }); } catch (error) { if (error.code === 'ENOENT') return []; throw error; } }
async function size(path) {
  const info = await lstat(path);
  if (info.isSymbolicLink()) return 0;
  if (!info.isDirectory()) return info.size;
  let total = 0;
  for (const entry of await entries(path)) total += await size(join(path, entry.name));
  return total;
}
export async function cleanBuilds({ root, version, dryRun = true, includeDebug = false, target }) {
  const workspace = await realpath(root);
  const candidates = [];
  const parsed = version.match(/^(\d+)\.(\d+)\.(\d+)(?:-alpha\.(\d+))?$/);
  if (!parsed) throw new Error('Cleanup requires a valid release version.');
  function isOlder(name) {
    const found = name.match(/\d+\.\d+\.\d+(?:-alpha\.\d+)?/)?.[0];
    if (!found) return false;
    const parts = value => { const [release, alpha] = value.split('-alpha.'); return [...release.split('.').map(Number), alpha === undefined ? Infinity : Number(alpha)]; };
    const a = parts(found), b = parts(version);
    for (let i = 0; i < a.length; i++) { if (a[i] !== b[i]) return a[i] < b[i]; }
    return false;
  }
  const releases=[releaseDirectory(workspace),...(target?[releaseDirectory(workspace,target)]:[])];
  for (const directory of releases) for (const entry of await entries(directory)) {
    if (/^ScreenTranslate-.*\.exe$/.test(entry.name) && isOlder(entry.name)) candidates.push(join(directory, entry.name));
  }
  async function bundles(directory) {
    for (const entry of await entries(directory)) {
      const path = join(directory, entry.name);
      if (entry.isSymbolicLink()) continue;
      if (entry.isDirectory()) await bundles(path);
      else if (/\.(exe|msi|dmg|deb|rpm|AppImage|zip|gz|sig)$/i.test(entry.name) && /\d+\.\d+\.\d+/.test(entry.name) && isOlder(entry.name)) candidates.push(path);
    }
  }
  for (const directory of releases) await bundles(join(directory,'bundle'));
  for (const entry of await entries(join(workspace, 'build'))) {
    const alpha = entry.name.match(/^alpha(\d+)[-.]/);
    if (alpha && isOlder('2.0.0-alpha.' + alpha[1])) candidates.push(join(workspace, 'build', entry.name));
    const release = entry.name.match(/^release-(\d+\.\d+\.\d+(?:-alpha\.\d+)?)(?:$|-)/);
    if (release && isOlder(release[1])) candidates.push(join(workspace, 'build', entry.name));
  }
  // Active build caches are never cleanup candidates, including debug junctions.
  const report = { version, dryRun, removedBytes: 0, plannedBytes: 0, items: [], skipped: [] };
  for (const path of candidates) {
    try {
      const absolute = await realpath(path);
      const location = relative(workspace, absolute);
      if (!location || location.startsWith('..') || isAbsolute(location)) throw new Error('Resolved path leaves the workspace.');
      // Only these generated locations can reach recursive removal.
      if (!(releases.some(directory=>location.replaceAll('\\','/').startsWith(relative(workspace,directory).replaceAll('\\','/')+'/')) || location.replaceAll('\\', '/').startsWith('build/'))) throw new Error('Unexpected cleanup location.');
      const bytes = await size(path);
      report.plannedBytes += bytes;
      report.items.push({ path: absolute, bytes });
      if (!dryRun) { await rm(absolute, { recursive: true, force: true, maxRetries: 2 }); report.removedBytes += bytes; }
    } catch (error) { report.skipped.push({ path, reason: error.message }); }
  }
  return report;
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const root = fileURLToPath(new URL('../', import.meta.url));
  const { version } = JSON.parse(await (await import('node:fs/promises')).readFile(join(root, 'apps/desktop/package.json'), 'utf8'));
  const report = await cleanBuilds({ root, version, dryRun: !process.argv.includes('--apply'), includeDebug: process.argv.includes('--debug-cache'), target: buildTarget(process.argv.slice(2)) });
  await mkdir(join(root, `build/release-${version}`), { recursive: true });
  await writeFile(join(root, `build/release-${version}/cleanup.json`), JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report, null, 2));
}
