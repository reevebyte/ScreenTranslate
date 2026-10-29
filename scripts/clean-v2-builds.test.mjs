import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, stat, rm, symlink } from 'node:fs/promises';
import { join, resolve, relative, isAbsolute } from 'node:path';
import { cleanBuilds } from './clean-v2-builds.mjs';

// A fresh checkout does not contain the ignored build directory.
await mkdir(resolve('build'), { recursive: true });
test('cleanup keeps the current version and source and rejects a junction outside the workspace', async () => {
  const base = await mkdtemp(resolve('build/alpha8-cleanup-check-'));
  const root = join(base, 'workspace');
  const outside = join(base, 'outside');
  try {
    for (const folder of ['build/alpha7-native', 'build/alpha8-native', 'target/release/bundle/nsis', 'src']) await mkdir(join(root, folder), { recursive: true });
    await mkdir(outside); await writeFile(join(outside, 'keep.txt'), 'keep');
    await symlink(outside, join(root, 'target/release/ScreenTranslate-2.0.0-alpha.7.exe'), process.platform === 'win32' ? 'junction' : 'dir');
    for (const name of ['ScreenTranslate_2.0.0-alpha.7.exe', 'ScreenTranslate_2.0.0-alpha.8.exe']) await writeFile(join(root, 'target/release/bundle/nsis', name), 'binary');
    await writeFile(join(root, 'src/main.rs'), 'source');
    const plan = await cleanBuilds({ root, version: '2.0.0-alpha.8', includeDebug: true });
    assert.equal(plan.skipped.length, 1);
    assert.ok(await stat(join(root, 'build/alpha7-native')));
    await cleanBuilds({ root, version: '2.0.0-alpha.8', dryRun: false });
    assert.equal(await stat(join(root, 'build/alpha7-native')).catch(() => null), null);
    assert.ok(await stat(join(root, 'build/alpha8-native')));
    assert.ok(await stat(join(root, 'target/release/bundle/nsis/ScreenTranslate_2.0.0-alpha.8.exe')));
    assert.ok(await stat(join(root, 'src/main.rs')));
    assert.ok(await stat(join(outside, 'keep.txt')));
  } finally {
    const location = relative(resolve('build'), base);
    assert.ok(location && !location.startsWith('..') && !isAbsolute(location));
    await rm(base, { recursive: true, force: true });
  }
});

test('cleanup preserves future versions, current artifacts, unversioned files and debug cache', async () => {
  const root = await mkdtemp(resolve('build/cleanup-versions-'));
  try {
    await mkdir(join(root, 'target/release/bundle/nsis'), { recursive: true });
    await mkdir(join(root, 'target/debug'), { recursive: true });
    for (const version of ['1', '10', '11']) await writeFile(join(root, 'target/release', 'ScreenTranslate-2.0.0-alpha.' + version + '.exe'), version);
    await writeFile(join(root, 'target/release/screentranslate.exe'), 'current');
    await cleanBuilds({ root, version: '2.0.0-alpha.10', dryRun: false });
    assert.equal(await stat(join(root, 'target/release/ScreenTranslate-2.0.0-alpha.1.exe')).catch(() => null), null);
    for (const name of ['ScreenTranslate-2.0.0-alpha.10.exe', 'ScreenTranslate-2.0.0-alpha.11.exe', 'screentranslate.exe']) assert.ok(await stat(join(root, 'target/release', name)));
    assert.ok(await stat(join(root, 'target/debug')));
  } finally {
    const location = relative(resolve('build'), root);
    assert.ok(location && !location.startsWith('..') && !isAbsolute(location));
    await rm(root, { recursive: true, force: true });
  }
});

test('stable releases remove prereleases but preserve current, future, unversioned artifacts and caches',async()=>{
  const root=await mkdtemp(resolve('build/stable-cleanup-check-'));
  try {
    await mkdir(join(root,'target/release/bundle/nsis'),{recursive:true});await mkdir(join(root,'target/debug'),{recursive:true});
    for(const version of ['2.0.0-alpha.23','2.0.0','2.0.1','2.1.0-alpha.1'])await writeFile(join(root,'target/release','ScreenTranslate-'+version+'.exe'),'binary');
    await writeFile(join(root,'target/release/screentranslate.exe'),'current');
    await cleanBuilds({root,version:'2.0.0',dryRun:false,includeDebug:true});
    assert.equal(await stat(join(root,'target/release/ScreenTranslate-2.0.0-alpha.23.exe')).catch(()=>null),null);
    for(const version of ['2.0.0','2.0.1','2.1.0-alpha.1'])assert.ok(await stat(join(root,'target/release','ScreenTranslate-'+version+'.exe')));
    assert.ok(await stat(join(root,'target/debug')));assert.ok(await stat(join(root,'target/release/screentranslate.exe')));
  }finally{const location=relative(resolve('build'),root);assert.ok(location&&!location.startsWith('..')&&!isAbsolute(location));await rm(root,{recursive:true,force:true});}
});
