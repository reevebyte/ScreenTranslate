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
function releaseAssets(release,tag) {
  if(!/^v2\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(tag??''))throw new Error('Invalid v2 release tag');
  if(release.tag_name!==tag||!Number.isSafeInteger(release.id)||release.id<=0||!Array.isArray(release.assets))throw new Error('Release asset catalog does not match the requested tag');
  if(release.draft!==true)throw new Error('Refusing to change a published release');
  const assets=new Map();
  for(const asset of release.assets) {
    if(typeof asset.name!=='string'||!asset.name||/[\\/]/.test(asset.name)||asset.name==='.'||asset.name==='..'||assets.has(asset.name))throw new Error('Invalid or duplicate release asset name');
    assets.set(asset.name,asset);
  }
  return assets;
}
export function normalizeUpdateManifest(manifest,release,tag) {
  if(!tag||`v${manifest.version}`!==tag||!manifest.platforms||!Object.keys(manifest.platforms).length)throw new Error('Invalid v2 update manifest');
  const assets=releaseAssets(release,tag);
  const result=structuredClone(manifest);
  for(const [platform,artifact] of Object.entries(result.platforms)) {
    const url=githubUrl(artifact.url);
    const name=decodeURIComponent(url.pathname.split('/').at(-1));
    const asset=assets.get(name);
    const signature=assets.get(name+'.sig');
    if(!asset||!signature||typeof artifact.signature!=='string'||!artifact.signature.trim())throw new Error(`Missing signed release asset for ${platform}`);
    const catalogUrl=githubUrl(asset.browser_download_url);
    const signatureUrl=githubUrl(signature.browser_download_url);
    if(signatureUrl.href!==catalogUrl.href+'.sig')throw new Error(`Signature asset belongs to another release: ${platform}`);
    const canonical=new URL(`https://github.com${downloadPrefix}${encodeURIComponent(tag)}/${encodeURIComponent(name)}`);
    const latest=new URL(`https://github.com${latestDownloadPrefix}${encodeURIComponent(name)}`);
    // tauri-action uses latest/download when given releaseId without tagName.
    // Bind that alias to this release's catalog and signed sidecar, never follow it.
    if(url.href!==catalogUrl.href&&url.href!==canonical.href&&url.href!==latest.href)throw new Error(`Update asset belongs to another release: ${platform}`);
    artifact.url=canonical.href;
  }
  return result;
}
export async function manifestFromAssets(directory,release,tag,configuration) {
  const assets=releaseAssets(release,tag);
  if(configuration.version!==tag.slice(1)||typeof configuration.productName!=='string')throw new Error('Release configuration does not match the requested tag');
  // Match tauri-action's asset-name normalization, including the macOS architecture suffix.
  const name=configuration.productName.trim().replace(/[ ()[\]{}]/g,'.').replace(/\.\./g,'.').normalize('NFD').replace(/[\u0300-\u036f]/g,'');
  if(!name||/[\\/]/.test(name))throw new Error('Invalid product asset name');
  const version=configuration.version;
  const definitions=[
    {file:`${name}_${version}_x64-setup.exe`,platforms:['windows-x86_64','windows-x86_64-nsis']},
    {file:`${name}_${version}_amd64.AppImage`,platforms:['linux-x86_64','linux-x86_64-appimage']},
    {file:`${name}_x64.app.tar.gz`,platforms:['darwin-x86_64','darwin-x86_64-app']},
    {file:`${name}_aarch64.app.tar.gz`,platforms:['darwin-aarch64','darwin-aarch64-app']},
  ];
  const optional=[
    {platform:'windows-x86_64-msi',match:file=>file.startsWith(`${name}_${version}_x64_`)&&file.endsWith('.msi')},
    {platform:'linux-x86_64-deb',match:file=>file===`${name}_${version}_amd64.deb`},
    {platform:'linux-x86_64-rpm',match:file=>file.startsWith(`${name}-${version}-`)&&file.endsWith('.x86_64.rpm')},
  ];
  for(const {platform,match} of optional) {
    const candidates=[...assets.keys()].filter(match);
    if(candidates.length>1)throw new Error(`Ambiguous release assets for ${platform}`);
    if(candidates.length)definitions.push({file:candidates[0],platforms:[platform]});
  }
  const result={version,notes:release.body??'',pub_date:release.created_at??new Date().toISOString(),platforms:{}};
  for(const {file,platforms} of definitions) {
    if(!assets.has(file)||!assets.has(file+'.sig'))throw new Error(`Missing signed release asset for ${platforms[0]}: ${file}`);
    const signature=await readFile(resolve(directory,file+'.sig'),'utf8');
    if(!signature.trim())throw new Error(`Empty signature: ${file}`);
    for(const platform of platforms)result.platforms[platform]={url:assets.get(file).browser_download_url,signature};
  }
  return normalizeUpdateManifest(result,release,tag);
}
export async function prepareManifest(directory,tag,{fromAssets=false,configuration}={}) {
  const release=JSON.parse(await readFile(resolve(directory,'release.json'),'utf8'));
  if(process.env.RELEASE_ID&&String(release.id)!==process.env.RELEASE_ID)throw new Error('Unexpected release ID');
  const manifest=fromAssets
    ?await manifestFromAssets(directory,release,tag,configuration??JSON.parse(await readFile(new URL('../apps/desktop/src-tauri/tauri.conf.json',import.meta.url),'utf8')))
    :JSON.parse(await readFile(resolve(directory,'latest.json'),'utf8'));
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
if(process.argv[1]&&fileURLToPath(import.meta.url)===resolve(process.argv[1]))await prepareManifest(resolve(process.argv[2]||'build/v2-release'),process.env.RELEASE_TAG,{fromAssets:process.argv.includes('--from-assets')});
