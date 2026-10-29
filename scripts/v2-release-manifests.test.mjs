import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile, access, rm, realpath } from 'node:fs/promises';
import { join, relative, isAbsolute } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
const script=fileURLToPath(new URL('./v2-release-manifests.mjs',import.meta.url));
const base=fileURLToPath(new URL('../build/',import.meta.url));
async function fixture(fn) { await mkdir(base,{recursive:true});const directory=await mkdtemp(join(base,'update-manifest-test-'));try{await fn(directory);}finally{const location=relative(await realpath(base),await realpath(directory));assert.ok(location&&!location.startsWith('..')&&!isAbsolute(location));await rm(directory,{recursive:true,force:true});} }
const stable='https://github.com/reevebyte/ScreenTranslate/releases/download/v2.0.2/setup.exe';
const draft='https://github.com/reevebyte/ScreenTranslate/releases/download/untagged-fixture/setup.exe';
const manifest=(url=stable)=>({version:'2.0.2',notes:'Notes\nSecond line',platforms:{'windows-x86_64':{url,signature:'synthetic-test-signature'}}});
const catalog=()=>({id:99,tag_name:'v2.0.2',draft:true,assets:[{id:1,name:'setup.exe',browser_download_url:draft},{id:2,name:'setup.exe.sig',browser_download_url:draft+'.sig'}]});
async function input(directory,m=manifest(),release=catalog()){
 await writeFile(join(directory,'latest.json'),JSON.stringify(m));await writeFile(join(directory,'release.json'),JSON.stringify(release));await writeFile(join(directory,'setup.exe.sig'),'synthetic-test-signature\n');
}
const run=directory=>spawnSync(process.execPath,[script,directory],{env:{...process.env,RELEASE_TAG:'v2.0.2',RELEASE_ID:'99'},encoding:'utf8'});
test('writes both stable manifests without changing signatures or notes',()=>fixture(async directory=>{
 const value=manifest();await input(directory,value);const result=run(directory);assert.equal(result.status,0,result.stderr);
 for(const name of ['latest-v2.json','latest.json'])assert.deepEqual(JSON.parse(await readFile(join(directory,name),'utf8')),value);
 await assert.rejects(access(join(directory,'update-manifest.json')));
}));
test('normalizes an authenticated draft asset URL to its post-publication tag URL',()=>fixture(async directory=>{
 await input(directory,manifest(draft));const result=run(directory);assert.equal(result.status,0,result.stderr);
 const prepared=JSON.parse(await readFile(join(directory,'latest-v2.json'),'utf8'));assert.equal(prepared.platforms['windows-x86_64'].url,stable);assert.equal(prepared.platforms['windows-x86_64'].signature,'synthetic-test-signature');
}));
test('rejects another release, repository, insecure or credential-bearing URLs and unsigned entries',()=>fixture(async directory=>{
 for(const change of [m=>m.version='2.0.3',m=>m.platforms['windows-x86_64'].signature='',m=>m.platforms['windows-x86_64'].url=stable.replace('https:','http:'),m=>m.platforms['windows-x86_64'].url=stable.replace('reevebyte/ScreenTranslate','other/repo'),m=>m.platforms['windows-x86_64'].url=draft.replace('untagged-fixture','untagged-another'),m=>m.platforms['windows-x86_64'].url=stable.replace('https://','https://user:secret@')]) {
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
