import { join } from 'node:path';
import { copyFile } from 'node:fs/promises';

export function buildTarget(args, environment = process.env) {
  const index=args.indexOf('--target');
  const inline=args.find(arg=>arg.startsWith('--target='));
  const value=index>=0?args[index+1]:inline!==undefined?inline.slice(9):environment.CARGO_BUILD_TARGET;
  if ((index>=0 || inline!==undefined)&&!value) throw new Error('Missing build target');
  if(value && (!/^[A-Za-z0-9_.-]+$/.test(value) || value==='.' || value==='..' || value.startsWith('--'))) throw new Error('Invalid build target');
  return value || undefined;
}
export function releaseDirectory(root, target) {
  if(target) buildTarget(['--target',target],{});
  return target?join(root,'target',target,'release'):join(root,'target','release');
}
export async function copyWindowsExecutable({root,args,version,environment=process.env}) {
  const target=buildTarget(args,environment),directory=releaseDirectory(root,target);
  const source=join(directory,'screentranslate.exe'),destination=join(directory,`ScreenTranslate-${version}.exe`);
  await copyFile(source,destination);
  return {target,directory,source,destination};
}
