import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, readFile, access, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
const script=fileURLToPath(new URL('./v2-release-manifests.mjs',import.meta.url));
async function fixture(fn) { const directory=await mkdtemp(join(tmpdir(),'screentrans-update-test-'));try{await fn(directory);}finally{await rm(directory,{recursive:true,force:true});} }
const manifest=()=>({version:'2.0.2',platforms:{'windows-x86_64':{url:'https://github.com/reevebyte/ScreenTranslate/releases/download/v2.0.2/setup.exe',signature:'synthetic-test-signature'}}});
const run=directory=>spawnSync(process.execPath,[script,directory],{env:{...process.env,RELEASE_TAG:'v2.0.2'},encoding:'utf8'});
test('writes the signed v2 manifest without a legacy network dependency',()=>fixture(async directory=>{
 const value=manifest();await writeFile(join(directory,'latest.json'),JSON.stringify(value));assert.equal(run(directory).status,0);
 assert.deepEqual(JSON.parse(await readFile(join(directory,'latest-v2.json'),'utf8')),value);
 await assert.rejects(access(join(directory,'update-manifest.json')));
}));
test('rejects unsigned assets, another repository, insecure URLs and a version mismatch',()=>fixture(async directory=>{
 for(const change of [m=>m.version='2.0.3',m=>m.platforms['windows-x86_64'].signature='',m=>m.platforms['windows-x86_64'].url='http://github.com/reevebyte/ScreenTranslate/releases/download/v2.0.2/setup.exe',m=>m.platforms['windows-x86_64'].url='https://github.com/other/repo/releases/download/v2.0.2/setup.exe']) {
  const value=manifest();change(value);await writeFile(join(directory,'latest.json'),JSON.stringify(value));assert.notEqual(run(directory).status,0);
  await assert.rejects(access(join(directory,'latest-v2.json')));
 }
}));
