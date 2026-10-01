import { readFileSync, writeFileSync } from 'node:fs';
const source=readFileSync(new URL('../../src/lib/server/settings.ts',import.meta.url),'utf8');
const literal=source.match(/export const SETTINGS = (\{[\s\S]*?\}) as const satisfies/)!;
if(!literal)throw new Error('Settings declaration not found');
// This literal comes from the checked-out source, never from a request or environment.
const values=Function(`return (${literal[1]})`)();
writeFileSync(new URL('./settings.json',import.meta.url),JSON.stringify(Object.entries(values).map(([key,spec])=>({key,...spec as object})),null,2)+'\n');
console.log(JSON.stringify({settings:Object.keys(values).length}));
