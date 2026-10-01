import { BUILTIN_WORDS } from '../../src/lib/server/name-filter-words';
import { mkdirSync, writeFileSync } from 'node:fs';
mkdirSync(new URL('../assets/',import.meta.url), {recursive:true});
writeFileSync(new URL('../assets/name-filter-words.json',import.meta.url),JSON.stringify(BUILTIN_WORDS,null,2)+'\n');
