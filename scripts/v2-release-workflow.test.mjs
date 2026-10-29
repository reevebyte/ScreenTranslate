import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { access, chmod, copyFile, mkdir, mkdtemp, readFile, realpath, rm, writeFile } from 'node:fs/promises';
import { dirname, isAbsolute, join, relative } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root=fileURLToPath(new URL('../',import.meta.url));
const workflow=await readFile(join(root,'.github/workflows/v2-release.yml'),'utf8');
const finalize=workflow.match(/name: Validate and write signed update metadata[\s\S]*?run: \|\r?\n([\s\S]*?)\s*$/)[1].split(/\r?\n/).map(line=>line.slice(10)).join('\n');
const catalog=JSON.parse(await readFile(new URL('./fixtures/v2-release-assets.json',import.meta.url),'utf8'));
function bashPath(){
 if(process.platform!=='win32')return '/bin/bash';
 const gitPaths=spawnSync('where.exe',['git'],{encoding:'utf8'}).stdout?.trim().split(/\r?\n/)??[];
 return gitPaths.map(path=>join(dirname(path),'../bin/bash.exe')).find(existsSync);
}

// An offline GitHub CLI stand-in. No network, credentials, or real release changes.
const fakeCli=`#!/usr/bin/env node
const fs=require('node:fs');const path=require('node:path');
const args=process.argv.slice(2);const remote=path.resolve('remote');
const catalog=JSON.parse(fs.readFileSync(path.join(remote,'release.json'),'utf8'));
if(args[0]==='api'){
 if(args[1]!==\`repos/reevebyte/ScreenTranslate/releases/\${catalog.id}\`)throw Error('Unexpected release API path');
 process.stdout.write(JSON.stringify(catalog));
}else if(args[0]==='release'&&args[1]==='download'){
 if(args[2]!==catalog.tag_name)throw Error('Wrong download tag');
 const patterns=args.flatMap((arg,i)=>arg==='--pattern'?[args[i+1]]:[]);const dir=args[args.indexOf('--dir')+1];fs.mkdirSync(dir,{recursive:true});
 const names=fs.readdirSync(remote).filter(name=>patterns.some(p=>p==='*.sig'?name.endsWith('.sig'):p===name));
 if(!names.length)throw Error('No matching release assets');
 for(const name of names){fs.copyFileSync(path.join(remote,name),path.join(dir,name));if(process.env.CORRUPT_READBACK==='true'&&dir.endsWith('/verify')&&name==='latest-v2.json')fs.appendFileSync(path.join(dir,name),'corrupted');}
}else if(args[0]==='release'&&args[1]==='upload'){
 if(args[2]!==catalog.tag_name||!args.includes('--clobber'))throw Error('Wrong upload arguments');
 const files=args.slice(3).filter(arg=>arg!=='--clobber');
 fs.writeFileSync('upload-log.json',JSON.stringify(files));
 for(const file of files){if(/\\.(exe|sig|AppImage|gz)$/.test(file))throw Error('Must not replace installers or signatures');fs.copyFileSync(file,path.join(remote,path.basename(file)));}
}else throw Error('Unexpected gh command '+args.join(' '));
`;

async function fixture(t,fn){
 const bash=bashPath();if(!bash){t.skip('Git Bash is required on Windows to execute the actual workflow shell');return;}
 const base=join(root,'build');await mkdir(base,{recursive:true});const dir=await mkdtemp(join(base,'release-workflow-test-'));
 try {
  for(const folder of ['scripts','apps/desktop/src-tauri','docs','bin','remote'])await mkdir(join(dir,folder),{recursive:true});
  await copyFile(join(root,'scripts/v2-release-manifests.mjs'),join(dir,'scripts/v2-release-manifests.mjs'));
  await copyFile(join(root,'scripts/fixtures/v2-release-config.json'),join(dir,'apps/desktop/src-tauri/tauri.conf.json'));
  await writeFile(join(dir,'LICENSE'),'Fixture license');await writeFile(join(dir,'docs/v2-third-party-notices.txt'),'Fixture notices');
  await writeFile(join(dir,'remote/release.json'),JSON.stringify(catalog));
  for(const asset of catalog.assets.filter(a=>a.name.endsWith('.sig')))await writeFile(join(dir,'remote',asset.name),'synthetic-'+asset.name+'\n');
  await writeFile(join(dir,'remote/latest.json'),'{intentionally corrupt legacy manifest');
  await writeFile(join(dir,'bin/gh'),fakeCli);await chmod(join(dir,'bin/gh'),0o755);
  const run=(extra={})=>spawnSync(bash,['--noprofile','--norc','-e','-o','pipefail','-c',finalize],{cwd:dir,encoding:'utf8',env:{...process.env,PATH:join(dir,'bin')+(process.platform==='win32'?';':':')+process.env.PATH,GH_TOKEN:'synthetic-offline-token',GITHUB_REPOSITORY:'reevebyte/ScreenTranslate',RELEASE_ID:String(catalog.id),RELEASE_TAG:catalog.tag_name,...extra}});
  await fn(dir,run);
 }finally{
  const location=relative(await realpath(base),await realpath(dir));assert.ok(location&&!location.startsWith('..')&&!isAbsolute(location));await rm(dir,{recursive:true,force:true});
 }
}

test('actual final workflow shell generates, uploads and reads back all targets using the recorded draft catalog',t=>fixture(t,async(dir,run)=>{
 const result=run();assert.equal(result.status,0,result.stdout+'\n'+result.stderr);
 const uploaded=JSON.parse(await readFile(join(dir,'remote/latest-v2.json'),'utf8'));
 assert.equal(uploaded.version,'2.0.2');assert.equal(Object.keys(uploaded.platforms).length,11);
 assert.deepEqual(JSON.parse(await readFile(join(dir,'remote/latest.json'),'utf8')),uploaded);
 for(const platform of ['windows-x86_64','linux-x86_64','darwin-x86_64','darwin-aarch64'])assert.ok(uploaded.platforms[platform]);
 for(const entry of Object.values(uploaded.platforms))assert.match(entry.url,/\/releases\/download\/v2\.0\.2\//);
 const files=JSON.parse(await readFile(join(dir,'upload-log.json'),'utf8'));assert.equal(files.length,4);assert.ok(files.includes('LICENSE'));assert.ok(files.includes('docs/v2-third-party-notices.txt'));
}));

test('actual final workflow stops before upload when one platform signature is missing',t=>fixture(t,async(dir,run)=>{
 const changed=structuredClone(catalog);changed.assets=changed.assets.filter(a=>a.name!=='ScreenTranslate.2_aarch64.app.tar.gz.sig');await writeFile(join(dir,'remote/release.json'),JSON.stringify(changed));
 const result=run();assert.notEqual(result.status,0);assert.match(result.stderr,/darwin-aarch64/);await assert.rejects(access(join(dir,'upload-log.json')));
}));

test('actual final workflow fails if uploaded metadata differs on readback',t=>fixture(t,async(dir,run)=>{
 const result=run({CORRUPT_READBACK:'true'});assert.notEqual(result.status,0);await access(join(dir,'upload-log.json'));
}));
