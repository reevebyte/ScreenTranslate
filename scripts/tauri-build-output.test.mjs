import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdir,mkdtemp,writeFile,readFile,copyFile,rm,realpath,stat} from 'node:fs/promises';
import {join,relative,isAbsolute} from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
import {buildTarget,releaseDirectory,copyWindowsExecutable} from './tauri-build-output.mjs';
import {cleanBuilds} from './clean-v2-builds.mjs';
const workspace=fileURLToPath(new URL('../',import.meta.url));
const target='x86_64-pc-windows-msvc';
async function fixture(fn){const base=join(workspace,'build');await mkdir(base,{recursive:true});const root=await mkdtemp(join(base,'target-output-check-'));try{await fn(root);}finally{const location=relative(await realpath(base),await realpath(root));assert.ok(location&&!location.startsWith('..')&&!isAbsolute(location));await rm(root,{recursive:true,force:true});}}
test('resolves native, explicit, equals-form and environment Cargo targets safely',()=>{
 assert.equal(buildTarget(['build'],{}),undefined);
 for(const args of [['build','--target',target],['build','--target='+target]])assert.equal(buildTarget(args,{}),target);
 assert.equal(buildTarget(['build'],{CARGO_BUILD_TARGET:target}),target);
 for(const args of [['--target'],['--target='],['--target','../outside'],['--target','..'],['--target','--debug']])assert.throws(()=>buildTarget(args,{}));
});
test('copies the target binary rather than a stale native binary',()=>fixture(async root=>{
 const native=releaseDirectory(root),cross=releaseDirectory(root,target);await mkdir(native,{recursive:true});await mkdir(cross,{recursive:true});
 await writeFile(join(native,'screentranslate.exe'),'stale-native');await writeFile(join(cross,'screentranslate.exe'),'target-binary');
 const copied=await copyWindowsExecutable({root,args:['build','--target',target],version:'2.0.2',environment:{}});
 assert.equal(await readFile(copied.destination,'utf8'),'target-binary');
 await assert.rejects(copyWindowsExecutable({root,args:['build','--target','aarch64-pc-windows-msvc'],version:'2.0.2',environment:{}}),{code:'ENOENT'});
}));
test('cleans older triple-specific outputs while preserving current artifacts and caches',()=>fixture(async root=>{
 const cross=releaseDirectory(root,target);await mkdir(join(cross,'bundle/nsis'),{recursive:true});await mkdir(join(cross,'deps'),{recursive:true});
 for(const version of ['2.0.1','2.0.2','2.0.3']){await writeFile(join(cross,`ScreenTranslate-${version}.exe`),'binary');await writeFile(join(cross,'bundle/nsis',`ScreenTranslate_${version}_setup.exe`),'bundle');}
 await writeFile(join(cross,'screentranslate.exe'),'active');
 const report=await cleanBuilds({root,version:'2.0.2',target,dryRun:false});assert.equal(report.skipped.length,0);
 assert.equal(await stat(join(cross,'ScreenTranslate-2.0.1.exe')).catch(()=>null),null);
 for(const name of ['ScreenTranslate-2.0.2.exe','ScreenTranslate-2.0.3.exe','screentranslate.exe','deps'])assert.ok(await stat(join(cross,name)));
}));
test('release wrapper succeeds after a target build and never cleans on failed builds',()=>fixture(async root=>{
 const scripts=join(root,'scripts');await mkdir(scripts);await mkdir(join(root,'apps/desktop/src-tauri'),{recursive:true});
 // Simulate an old tagged source checkout with the workflow-owned driver overlaid.
 for(const name of ['tauri-v2.mjs','tauri-build-output.mjs','clean-v2-builds.mjs','check-v2-version.mjs'])await copyFile(join(workspace,'scripts',name),join(scripts,name));
 await writeFile(join(root,'Cargo.toml'),'version = "2.0.2"');await writeFile(join(root,'apps/desktop/package.json'),JSON.stringify({version:'2.0.2'}));
 await writeFile(join(root,'apps/desktop/src-tauri/updater.pub'),'synthetic-public-key');await writeFile(join(root,'apps/desktop/src-tauri/tauri.conf.json'),JSON.stringify({version:'2.0.2',plugins:{updater:{pubkey:'synthetic-public-key'}}}));
 const cli=join(root,'node_modules/@tauri-apps/cli');await mkdir(cli,{recursive:true});await writeFile(join(cli,'package.json'),JSON.stringify({name:'@tauri-apps/cli',type:'module',exports:'./index.js'}));await writeFile(join(cli,'index.js'),'');
 await writeFile(join(cli,'tauri.js'),`import {mkdir,writeFile} from 'node:fs/promises';import {join} from 'node:path';if(process.env.FAKE_BUILD_FAIL==='true')process.exit(7);const root=process.env.FAKE_WORKSPACE;const dir=join(root,'target',${JSON.stringify(target)},'release');await mkdir(dir,{recursive:true});await writeFile(join(dir,'screentranslate.exe'),'tagged-application');`);
 const cross=releaseDirectory(root,target);await mkdir(cross,{recursive:true});const old=join(cross,'ScreenTranslate-2.0.1.exe');await writeFile(old,'old');
 const args=[join(scripts,'tauri-v2.mjs'),'build','--target',target],env={...process.env,FAKE_WORKSPACE:root,SCREENTRANS_UPDATE_PUBLIC_KEY:'',RELEASE_TAG:'v2.0.2',CARGO_BUILD_TARGET:'',VITE_DESKTOP_E2E:'false'};
 let result=spawnSync(process.execPath,args,{cwd:join(root,'apps/desktop'),env:{...env,FAKE_BUILD_FAIL:'true'},encoding:'utf8'});assert.equal(result.status,7,result.stderr);assert.ok(await stat(old));
 result=spawnSync(process.execPath,args,{cwd:join(root,'apps/desktop'),env:{...env,FAKE_BUILD_FAIL:'false'},encoding:'utf8'});assert.equal(result.status,0,result.stderr);
 if(process.platform==='win32')assert.equal(await readFile(join(cross,'ScreenTranslate-2.0.2.exe'),'utf8'),'tagged-application');
 assert.equal(await stat(old).catch(()=>null),null);assert.ok(await stat(join(root,'build/release-2.0.2/cleanup.json')));
}));
