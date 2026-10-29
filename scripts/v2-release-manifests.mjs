import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const directory = resolve(process.argv[2] || 'build/v2-release');
const manifest = JSON.parse(await readFile(resolve(directory, 'latest.json'), 'utf8'));
const tag = process.env.RELEASE_TAG;
if (!tag || `v${manifest.version}` !== tag || !manifest.platforms || !Object.keys(manifest.platforms).length) throw new Error('Invalid v2 update manifest');
for (const artifact of Object.values(manifest.platforms)) {
  const url = new URL(artifact.url);
  if (url.origin !== 'https://github.com' || !url.pathname.startsWith(`/reevebyte/ScreenTranslate/releases/download/${tag}/`) || !artifact.signature) {
    throw new Error('Update artifacts must be signed assets of this repository and release');
  }
}
await writeFile(resolve(directory, 'latest-v2.json'), JSON.stringify(manifest, null, 2));
