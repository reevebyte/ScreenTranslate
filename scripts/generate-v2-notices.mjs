import { execFileSync } from 'node:child_process';
import { readFile, readdir, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

// Run through `pnpm notices` so pnpm's exact installed entry point is available on Windows too.
const root = fileURLToPath(new URL('../', import.meta.url));
if (!process.env.npm_execpath) throw new Error('Run with pnpm notices');
const cargo = JSON.parse(execFileSync('cargo', ['metadata', '--locked', '--format-version', '1'], { cwd: root, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 }));
const npm = JSON.parse(execFileSync(process.execPath, [process.env.npm_execpath, 'list', '-r', '--prod', '--depth', 'Infinity', '--json'], { cwd: root, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 }));
const selected = new Set();
const nodes = new Map(cargo.resolve.nodes.map(node => [node.id, node]));
function visitRust(id) {
  if (selected.has(id)) return;
  selected.add(id);
  for (const dependency of nodes.get(id)?.deps || []) {
    if (dependency.dep_kinds.some(kind => kind.kind !== 'dev')) visitRust(dependency.pkg);
  }
}
cargo.workspace_members.forEach(visitRust);
const packages = cargo.packages.filter(pkg => pkg.source && selected.has(pkg.id)).map(pkg => ({ name: pkg.name, version: pkg.version, license: pkg.license, repository: pkg.repository, path: dirname(pkg.manifest_path) }));
const seenNpm = new Set();
function visitNpm(tree) {
  for (const pkg of Object.values(tree.dependencies || {})) {
    const id = `${pkg.name || pkg.from}@${pkg.version}`;
    if (!seenNpm.has(id) && pkg.path) { seenNpm.add(id); packages.push({ ...pkg, name: pkg.name || pkg.from }); }
    visitNpm(pkg);
  }
}
npm.forEach(visitNpm);
const sections = ['ScreenTranslate — third-party notices\n\nGenerated from the locked Rust default-feature dependency graph and production frontend dependency graph. Includes build dependencies and a superset of platform dependencies. Optional E2E drivers are excluded. Each dependency retains its own license. System libraries and separately installed OCR language data have their own distribution terms.\n'];
sections.push('\nVendored UI components: shadcn-svelte (Nova)\nhttps://github.com/huntabyte/shadcn-svelte\n\n' + await readFile(join(root, 'apps/desktop/src/lib/components/ui/SHADCN-LICENSE.md'), 'utf8'));
sections.push('\nBundled provider artwork: @lobehub/icons-static-svg 1.101.0\nhttps://github.com/lobehub/lobe-icons\n\n' + await readFile(join(root, 'apps/desktop/src/lib/assets/providers/LOBE-LICENSE'), 'utf8'));
for (const pkg of packages.sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`))) {
  let directory;
  try { directory = await readdir(pkg.path); }
  catch (error) {
    // pnpm can list optional binaries for platforms that are not installed locally.
    if (error.code === 'ENOENT') continue;
    throw error;
  }
  let manifest = {};
  try { manifest = JSON.parse(await readFile(join(pkg.path, 'package.json'), 'utf8')); } catch {}
  const license = pkg.license || manifest.license || 'See upstream license';
  const files = directory.filter(name => /^(licen[sc]e|notice|copying|copyright)([._-]|$)/i.test(name));
  const texts = [];
  for (const file of files.sort()) {
    try { const text = await readFile(join(pkg.path, file), 'utf8'); if (text.length < 300_000) texts.push(`${file}:\n${text.trim()}`); } catch { /* Some packages use a license directory. */ }
  }
  sections.push(`\n${'='.repeat(72)}\n${pkg.name} ${pkg.version}\nLicense: ${typeof license === 'string' ? license : JSON.stringify(license)}\n${pkg.repository || manifest.homepage || ''}\n\n${texts.join('\n\n') || 'License text is distributed by the upstream package; refer to its SPDX license and repository.'}\n`);
}
await writeFile(join(root, 'docs/v2-third-party-notices.txt'), sections.join(''));
console.log(`Wrote notices for ${packages.length} dependencies.`);
