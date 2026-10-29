import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile, rm, realpath } from 'node:fs/promises';
import { dirname, join, relative, isAbsolute } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
const root=fileURLToPath(new URL('../',import.meta.url));
const workflow=await readFile(join(root,'.github/workflows/v2-release.yml'),'utf8');
const code=workflow.match(/node --input-type=module <<'RELEASE_SCRIPT'\r?\n([\s\S]*?)\r?\n          RELEASE_SCRIPT/)[1].split(/\r?\n/).map(line=>line.slice(10)).join('\n');
async function fixture(fn){
 const base=join(root,'build');await mkdir(base,{recursive:true});const dir=await mkdtemp(join(base,'release-script-check-'));
 try {await mkdir(join(dir,'docs'));await writeFile(join(dir,'docs/v2-release-notes.md'),'Fixture release notes\nSecond line');await fn(dir);}
 finally {const location=relative(await realpath(base),await realpath(dir));assert.ok(location&&!location.startsWith('..')&&!isAbsolute(location));await rm(dir,{recursive:true,force:true});}
}
async function run(dir,pages,{post={id:99,tag_name:'v2.0.2',draft:true},status=200,finishOnly=false}={}){
 const stub=`const _calls=[];globalThis.fetch=async(url,options)=>{_calls.push({url,...options});await(await import('node:fs/promises')).writeFile('calls.json',JSON.stringify(_calls));return {ok:${status}>=200&&${status}<300,status:${status},json:async()=>options.method==='POST'?${JSON.stringify(post)}:${JSON.stringify(pages)}[Number(new URL(url).searchParams.get('page'))-1]??[]};};`;
 await writeFile(join(dir,'run.mjs'),stub+'\n'+code);
 const result=spawnSync(process.execPath,[join(dir,'run.mjs')],{cwd:dir,encoding:'utf8',env:{...process.env,GITHUB_REPOSITORY:'reevebyte/ScreenTranslate',RELEASE_TAG:'v2.0.2',GH_TOKEN:'synthetic-token',FINISH_ONLY:String(finishOnly),GITHUB_OUTPUT:join(dir,'output.txt')}});
 return {...result,calls:JSON.parse(await readFile(join(dir,'calls.json'),'utf8'))};
}
test('reuses an existing unpublished draft without the by-tag endpoint',()=>fixture(async dir=>{
 const result=await run(dir,[[{id:427,tag_name:'v2.0.2',draft:true}]]);assert.equal(result.status,0,result.stderr);assert.equal(result.calls.length,1);assert.equal(await readFile(join(dir,'output.txt'),'utf8'),'id=427\n');
}));
test('uses the creation response directly and preserves release-note newlines',()=>fixture(async dir=>{
 const result=await run(dir,[[]]);assert.equal(result.status,0,result.stderr);assert.deepEqual(result.calls.map(c=>c.method),['GET','POST']);assert.equal(JSON.parse(result.calls[1].body).body,'Fixture release notes\nSecond line');assert.equal(await readFile(join(dir,'output.txt'),'utf8'),'id=99\n');
}));
test('refuses published releases and API permission failures without creating another draft',()=>fixture(async dir=>{
 let result=await run(dir,[[{id:10,tag_name:'v2.0.2',draft:false}]]);assert.notEqual(result.status,0);assert.equal(result.calls.length,1);assert.match(result.stderr,/Refusing to change a published release/);
 result=await run(dir,[[]],{status:403});assert.notEqual(result.status,0);assert.equal(result.calls.length,1);assert.match(result.stderr,/HTTP 403/);
}));
test('finds the draft on a later release-list page',()=>fixture(async dir=>{
 const result=await run(dir,[Array.from({length:100},(_,i)=>({id:i+1,tag_name:'v1.'+i,draft:false})),[{id:427,tag_name:'v2.0.2',draft:true}]]);assert.equal(result.status,0,result.stderr);assert.equal(result.calls.length,2);
}));

test('metadata-only mode requires an existing draft and never creates an empty replacement',()=>fixture(async dir=>{
 let result=await run(dir,[[]],{finishOnly:true});assert.notEqual(result.status,0);assert.match(result.stderr,/requires an existing draft/);assert.deepEqual(result.calls.map(c=>c.method),['GET']);
 result=await run(dir,[[{id:427,tag_name:'v2.0.2',draft:true}]],{finishOnly:true});assert.equal(result.status,0,result.stderr);assert.equal(result.calls.length,1);
}));

test('metadata-only retry skips builds but still runs the guarded metadata job',()=>{
 assert.match(workflow,/finish_only:[\s\S]*?type: boolean/);
 assert.match(workflow,/build:\r?\n    if: \$\{\{ inputs\.finish_only != true \}\}/);
 assert.match(workflow,/manifests:[\s\S]*?always\(\)[\s\S]*?needs\.build\.result == 'skipped'/);
 assert.match(workflow,/Check out the metadata driver from the workflow revision/);
 assert.match(workflow,/node scripts\/v2-release-manifests\.mjs build\/v2-release --from-assets/);
 assert.match(workflow,/includeUpdaterJson: false/);
 assert.match(workflow,/cmp build\/v2-release\/latest-v2\.json build\/v2-release\/verify\/latest-v2\.json/);
});

test('Tauri receives the explicit release tag so updater URLs never fall back to latest',()=>{
 const action=workflow.match(/- uses: tauri-apps\/tauri-action@v0[\s\S]*?(?=\r?\n  manifests:)/)?.[0];
 assert.ok(action,'Missing Tauri release action');
 assert.match(action,/releaseId: \$\{\{ needs\.prepare\.outputs\.release_id \}\}/);
 assert.match(action,/tagName: \$\{\{ env\.RELEASE_TAG \}\}/);
});
