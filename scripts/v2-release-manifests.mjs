import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const repository='reevebyte/ScreenTranslate';
const downloadPrefix=`/${repository}/releases/download/`;
const latestDownloadPrefix=`/${repository}/releases/latest/download/`;
function githubUrl(value) {
  const url=new URL(value);
  if(url.origin!=='https://github.com'||url.username||url.password||url.search||url.hash||(!url.pathname.startsWith(downloadPrefix)&&!url.pathname.startsWith(latestDownloadPrefix)))throw new Error('Update URL must belong to this repository');
  return url;
}
export function normalizeUpdateManifest(manifest,release,tag) {
  if(!tag||`v${manifest.version}`!==tag||!manifest.platforms||!Object.keys(manifest.platforms).length)throw new Error('Invalid v2 update manifest');
  if(release.tag_name!==tag||!Number.isSafeInteger(release.id)||release.id<=0||!Array.isArray(release.assets))throw new Error('Release asset catalog does not match the requested tag');
  const assets=new Map();
  for(const asset of release.assets) {
    if(typeof asset.name!=='string'||!asset.name||/[\\/]/.test(asset.name)||asset.name==='.'||asset.name==='..'||assets.has(asset.name))throw new Error('Invalid or duplicate release asset name');
    assets.set(asset.name,asset);
  }
  const result=structuredClone(manifest);
  for(const [platform,artifact] of Object.entries(result.platforms)) {
    const url=githubUrl(artifact.url);
    const name=decodeURIComponent(url.pathname.split('/').at(-1));
    const asset=assets.get(name);
    const signature=assets.get(name+'.sig');
    if(!asset||!signature||typeof artifact.signature!=='string'||!artifact.signature.trim())throw new Error(`Missing signed release asset for ${platform}`);
    const catalogUrl=githubUrl(asset.browser_download_url);
    const canonical=new URL(`https://github.com${downloadPrefix}${encodeURIComponent(tag)}/${encodeURIComponent(name)}`);
    const latest=new URL(`https://github.com${latestDownloadPrefix}${encodeURIComponent(name)}`);
    // tauri-action uses latest/download when given releaseId without tagName.
    // Bind that alias to this release's catalog and signed sidecar, never follow it.
    if(url.href!==catalogUrl.href&&url.href!==canonical.href&&url.href!==latest.href)throw new Error(`Update asset belongs to another release: ${platform}`);
    artifact.url=canonical.href;
  }
  return result;
}
export async function prepareManifest(directory,tag) {
  const manifest=JSON.parse(await readFile(resolve(directory,'latest.json'),'utf8'));
  const release=JSON.parse(await readFile(resolve(directory,'release.json'),'utf8'));
  if(process.env.RELEASE_ID&&String(release.id)!==process.env.RELEASE_ID)throw new Error('Unexpected release ID');
  const normalized=normalizeUpdateManifest(manifest,release,tag);
  for(const [platform,artifact] of Object.entries(normalized.platforms)) {
    const name=decodeURIComponent(new URL(artifact.url).pathname.split('/').at(-1));
    const signature=(await readFile(resolve(directory,name+'.sig'),'utf8')).trim();
    if(signature!==artifact.signature.trim())throw new Error(`Signature differs from the uploaded sidecar: ${platform}`);
  }
  // Publish stable post-release URLs while preserving every original signature and platform field.
  const text=JSON.stringify(normalized,null,2);
  await writeFile(resolve(directory,'latest-v2.json'),text);
  await writeFile(resolve(directory,'latest.json'),text);
  console.log(`Prepared ${tag} update metadata for ${Object.keys(normalized.platforms).join(', ')}`);
  return normalized;
}
if(process.argv[1]&&fileURLToPath(import.meta.url)===resolve(process.argv[1]))await prepareManifest(resolve(process.argv[2]||'build/v2-release'),process.env.RELEASE_TAG);
