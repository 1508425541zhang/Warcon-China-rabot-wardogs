import {resolve} from 'node:path';
import {existsSync,statSync} from 'node:fs';
const root=resolve(import.meta.dir,'../..');
const result=await Bun.build({entrypoints:[resolve(import.meta.dir,'actions-entry.ts')],target:'bun',format:'esm',external:['bun','bun:*'],plugins:[{name:'original-action-env',setup(build){
 build.onResolve({filter:/^\$env\/dynamic\/private$/},()=>({path:'env',namespace:'fixture'}));
 build.onResolve({filter:/^\$app\/environment$/},()=>({path:'app',namespace:'fixture'}));
 build.onLoad({filter:/.*/,namespace:'fixture'},args=>({contents:args.path==='env'?'export const env = process.env;':'export const building=false;export const dev=false;export const browser=false;',loader:'js'}));
 build.onResolve({filter:/^\$lib\//},args=>{const base=resolve(root,'src/lib',args.path.slice(5));for(const path of [base,`${base}.ts`,`${base}/index.ts`])if(existsSync(path)&&statSync(path).isFile())return {path};return {path:base};});
}}]});
if(!result.success)throw new AggregateError(result.logs,'Fixture bundling failed');
const path=resolve(root,'backend/target-native/fixture-actions.mjs');await Bun.write(path,result.outputs[0]);
const child=Bun.spawn([process.execPath,path],{cwd:root,stdout:'inherit',stderr:'inherit'});if(await child.exited!==0)throw new Error('Original action fixture failed');
