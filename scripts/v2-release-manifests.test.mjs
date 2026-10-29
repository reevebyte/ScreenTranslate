import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, copyFile, writeFile, readFile, access, rm, realpath } from 'node:fs/promises';
import { join, relative, isAbsolute } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { manifestFromAssets } from './v2-release-manifests.mjs';
const script=fileURLToPath(new URL('./v2-release-manifests.mjs',import.meta.url));
const base=fileURLToPath(new URL('../build/',import.meta.url));
async function fixture(fn) { await mkdir(base,{recursive:true});const directory=await mkdtemp(join(base,'update-manifest-test-'));try{await fn(directory);}finally{const location=relative(await realpath(base),await realpath(directory));assert.ok(location&&!location.startsWith('..')&&!isAbsolute(location));await rm(directory,{recursive:true,force:true});} }
const stable='https://github.com/reevebyte/ScreenTranslate/releases/download/v2.0.2/setup.exe';
const draft='https://github.com/reevebyte/ScreenTranslate/releases/download/untagged-fixture/setup.exe';
const latest='https://github.com/reevebyte/ScreenTranslate/releases/latest/download/setup.exe';
const manifest=(url=stable)=>({version:'2.0.2',notes:'Notes\nSecond line',platforms:{'windows-x86_64':{url,signature:'synthetic-test-signature'}}});
const catalog=()=>({id:99,tag_name:'v2.0.2',draft:true,assets:[{id:1,name:'setup.exe',browser_download_url:draft},{id:2,name:'setup.exe.sig',browser_download_url:draft+'.sig'}]});
async function input(directory,m=manifest(),release=catalog()){
 await writeFile(join(directory,'latest.json'),JSON.stringify(m));await writeFile(join(directory,'release.json'),JSON.stringify(release));await writeFile(join(directory,'setup.exe.sig'),'synthetic-test-signature\n');
}
const run=(directory,...flags)=>spawnSync(process.execPath,[flags.includes('--from-assets')?join(directory,'scripts/v2-release-manifests.mjs'):script,directory,...flags],{env:{...process.env,RELEASE_TAG:'v2.0.2',RELEASE_ID:'99'},encoding:'utf8'});
const signedFiles=['ScreenTranslate.2_2.0.2_x64-setup.exe','ScreenTranslate.2_2.0.2_amd64.AppImage','ScreenTranslate.2_x64.app.tar.gz','ScreenTranslate.2_aarch64.app.tar.gz','ScreenTranslate.2_2.0.2_x64_en-US.msi','ScreenTranslate.2_2.0.2_amd64.deb','ScreenTranslate.2-2.0.2-1.x86_64.rpm'];
async function assetInput(directory){
 await mkdir(join(directory,'scripts'),{recursive:true});await mkdir(join(directory,'apps/desktop/src-tauri'),{recursive:true});
 await copyFile(script,join(directory,'scripts/v2-release-manifests.mjs'));await copyFile(new URL('./fixtures/v2-release-config.json',import.meta.url),join(directory,'apps/desktop/src-tauri/tauri.conf.json'));
 const release={...catalog(),body:'Release notes\nSecond line',created_at:'2026-10-29T10:00:00Z',assets:[]};
 for(const name of signedFiles){
  for(const suffix of ['', '.sig'])release.assets.push({name:name+suffix,browser_download_url:draft.replace('setup.exe',name+suffix)});
  await writeFile(join(directory,name+'.sig'),'synthetic-'+name+'\n');
 }
 await writeFile(join(directory,'release.json'),JSON.stringify(release));return release;
}

test('builds all four targets and installer aliases from the uploaded catalog without latest.json',()=>fixture(async directory=>{
 await assetInput(directory);const result=run(directory,'--from-assets');assert.equal(result.status,0,result.stderr);
 const prepared=JSON.parse(await readFile(join(directory,'latest-v2.json'),'utf8'));
 assert.equal(prepared.version,'2.0.2');assert.equal(prepared.notes,'Release notes\nSecond line');assert.equal(prepared.pub_date,'2026-10-29T10:00:00Z');
 assert.equal(Object.keys(prepared.platforms).length,11);
 for(const platform of ['windows-x86_64','linux-x86_64','darwin-x86_64','darwin-aarch64'])assert.ok(prepared.platforms[platform]);
 for(const entry of Object.values(prepared.platforms)){
  const name=new URL(entry.url).pathname.split('/').at(-1);assert.ok(signedFiles.includes(name));assert.equal(entry.url,stable.replace('setup.exe',name));assert.equal(entry.signature,await readFile(join(directory,name+'.sig'),'utf8'));
 }
 assert.deepEqual(JSON.parse(await readFile(join(directory,'latest.json'),'utf8')),prepared);
}));

test('asset mode recovers corrupt or partial legacy metadata and is repeatable',()=>fixture(async directory=>{
 await assetInput(directory);await writeFile(join(directory,'latest.json'),'{broken previous metadata');
 let result=run(directory,'--from-assets');assert.equal(result.status,0,result.stderr);const expected=await readFile(join(directory,'latest.json'),'utf8');
 result=run(directory,'--from-assets');assert.equal(result.status,0,result.stderr);assert.equal(await readFile(join(directory,'latest.json'),'utf8'),expected);
}));

test('asset mode refuses missing platform installers, missing or empty signatures, and published catalogs',async()=>{
 for(const change of [r=>r.assets=r.assets.filter(a=>a.name!==signedFiles[3]),r=>r.assets=r.assets.filter(a=>a.name!==signedFiles[2]+'.sig'),r=>r.draft=false,r=>r.assets[1].browser_download_url=draft.replace('untagged-fixture','other-release')])await fixture(async directory=>{
  const release=await assetInput(directory);change(release);await writeFile(join(directory,'release.json'),JSON.stringify(release));const result=run(directory,'--from-assets');assert.notEqual(result.status,0);await assert.rejects(access(join(directory,'latest-v2.json')));
 });
 await fixture(async directory=>{await assetInput(directory);await writeFile(join(directory,signedFiles[0]+'.sig'),'\n');assert.notEqual(run(directory,'--from-assets').status,0);await assert.rejects(access(join(directory,'latest.json')));});
});

test('asset generation binds the config version and rejects ambiguous installer variants',()=>fixture(async directory=>{
 const release=await assetInput(directory);await assert.rejects(manifestFromAssets(directory,release,'v2.0.2',{version:'2.0.3',productName:'ScreenTranslate 2'}),/configuration does not match/);
 release.assets.push({name:'ScreenTranslate.2_2.0.2_x64_zh-CN.msi'});await assert.rejects(manifestFromAssets(directory,release,'v2.0.2',{version:'2.0.2',productName:'ScreenTranslate 2'}),/Ambiguous/);
}));

test('builds the renamed product for the current release while retaining the historical release fixture',()=>fixture(async directory=>{
 const config=JSON.parse(await readFile(new URL('../apps/desktop/src-tauri/tauri.conf.json',import.meta.url),'utf8'));
 assert.equal(config.productName,'ScreenTranslate');
 const release={...catalog(),tag_name:'v'+config.version,assets:[]};
 for(const oldName of signedFiles){
  const name=oldName.replace('ScreenTranslate.2','ScreenTranslate').replaceAll('2.0.2',config.version);
  for(const suffix of ['', '.sig'])release.assets.push({name:name+suffix,browser_download_url:draft.replace('setup.exe',name+suffix)});
  await writeFile(join(directory,name+'.sig'),'synthetic-current-signature\n');
 }
 const value=await manifestFromAssets(directory,release,release.tag_name,config);
 assert.equal(Object.keys(value.platforms).length,11);
 assert.equal(new URL(value.platforms['windows-x86_64'].url).pathname.split('/').at(-1),`ScreenTranslate_${config.version}_x64-setup.exe`);
 assert.ok(Object.values(value.platforms).every(a=>!a.url.includes('ScreenTranslate.2')));
}));
test('writes both stable manifests without changing signatures or notes',()=>fixture(async directory=>{
 const value=manifest();await input(directory,value);const result=run(directory);assert.equal(result.status,0,result.stderr);
 for(const name of ['latest-v2.json','latest.json'])assert.deepEqual(JSON.parse(await readFile(join(directory,name),'utf8')),value);
 await assert.rejects(access(join(directory,'update-manifest.json')));
}));
test('normalizes an authenticated draft asset URL to its post-publication tag URL',()=>fixture(async directory=>{
 await input(directory,manifest(draft));const result=run(directory);assert.equal(result.status,0,result.stderr);
 const prepared=JSON.parse(await readFile(join(directory,'latest-v2.json'),'utf8'));assert.equal(prepared.platforms['windows-x86_64'].url,stable);assert.equal(prepared.platforms['windows-x86_64'].signature,'synthetic-test-signature');
}));
test('normalizes tauri-action releaseId-only latest/download URLs using the signed release catalog',()=>fixture(async directory=>{
 const entries=[
  ['windows-x86_64','ScreenTranslate.2_2.0.2_x64-setup.exe'],
  ['windows-x86_64-nsis','ScreenTranslate.2_2.0.2_x64-setup.exe'],
  ['windows-x86_64-msi','ScreenTranslate.2_2.0.2_x64_en-US.msi'],
  ['linux-x86_64','ScreenTranslate.2_2.0.2_amd64.AppImage'],
  ['linux-x86_64-appimage','ScreenTranslate.2_2.0.2_amd64.AppImage'],
  ['linux-x86_64-deb','ScreenTranslate.2_2.0.2_amd64.deb'],
  ['linux-x86_64-rpm','ScreenTranslate.2-2.0.2-1.x86_64.rpm'],
  ['darwin-x86_64','ScreenTranslate.2_x64.app.tar.gz'],
  ['darwin-x86_64-app','ScreenTranslate.2_x64.app.tar.gz'],
  ['darwin-aarch64','ScreenTranslate.2_aarch64.app.tar.gz'],
  ['darwin-aarch64-app','ScreenTranslate.2_aarch64.app.tar.gz'],
 ];
 const value={...manifest(),pub_date:'2026-10-29T10:00:00Z',platforms:{}};
 const release={...catalog(),assets:[]};
 for(const name of new Set(entries.map(([,name])=>name))){
  for(const suffix of ['', '.sig'])release.assets.push({name:name+suffix,browser_download_url:draft.replace('setup.exe',name+suffix)});
  await writeFile(join(directory,name+'.sig'),'synthetic-'+name+'\n');
 }
 for(const [platform,name] of entries)value.platforms[platform]={url:latest.replace('setup.exe',name),signature:'synthetic-'+name+'\n'};
 await writeFile(join(directory,'latest.json'),JSON.stringify(value));await writeFile(join(directory,'release.json'),JSON.stringify(release));
 const result=run(directory);assert.equal(result.status,0,result.stderr);
 const expected=structuredClone(value);for(const [platform,name] of entries)expected.platforms[platform].url=stable.replace('setup.exe',name);
 for(const file of ['latest.json','latest-v2.json'])assert.deepEqual(JSON.parse(await readFile(join(directory,file),'utf8')),expected);
}));
test('latest/download aliases still require this release assets and matching uploaded signatures',()=>fixture(async directory=>{
 for(const [release,signature] of [[{...catalog(),assets:[]},'synthetic-test-signature'],[catalog(),'mismatched-signature']]){
  const value=manifest(latest);await input(directory,value,release);await writeFile(join(directory,'setup.exe.sig'),signature);
  const result=run(directory);assert.notEqual(result.status,0);assert.deepEqual(JSON.parse(await readFile(join(directory,'latest.json'),'utf8')),value);await assert.rejects(access(join(directory,'latest-v2.json')));
 }
}));
test('rejects another release, repository, insecure or credential-bearing URLs and unsigned entries',()=>fixture(async directory=>{
 for(const change of [m=>m.version='2.0.3',m=>m.platforms['windows-x86_64'].signature='',...[stable.replace('https:','http:'),stable.replace('reevebyte/ScreenTranslate','other/repo'),draft.replace('untagged-fixture','untagged-another'),stable.replace('https://','https://user:secret@'),latest.replace('ScreenTranslate','OtherRepo'),latest.replace('/download/','/download/nested/'),latest+'?token=synthetic',latest+'#fragment',latest.replace('https:','http:'),latest.replace('https://','https://user:secret@')].map(url=>m=>m.platforms['windows-x86_64'].url=url)]) {
  const value=manifest();change(value);await input(directory,value);assert.notEqual(run(directory).status,0);await assert.rejects(access(join(directory,'latest-v2.json')));
 }
}));
test('rejects missing assets, signature sidecars, release IDs and catalog tag mismatches',()=>fixture(async directory=>{
 for(const change of [r=>r.assets.pop(),r=>r.assets=[],r=>r.id=100,r=>r.tag_name='v2.0.3',r=>r.assets[0].browser_download_url=draft.replace('reevebyte/ScreenTranslate','other/repo')]){
  const release=catalog();change(release);await input(directory,manifest(draft),release);assert.notEqual(run(directory).status,0);await assert.rejects(access(join(directory,'latest-v2.json')));
 }
}));
test('rejects a manifest signature that differs from the uploaded sidecar and preserves the input',()=>fixture(async directory=>{
 const value=manifest(draft);await input(directory,value);await writeFile(join(directory,'setup.exe.sig'),'different-signature');const result=run(directory);assert.notEqual(result.status,0);assert.match(result.stderr,/Signature differs/);assert.deepEqual(JSON.parse(await readFile(join(directory,'latest.json'),'utf8')),value);await assert.rejects(access(join(directory,'latest-v2.json')));
}));
