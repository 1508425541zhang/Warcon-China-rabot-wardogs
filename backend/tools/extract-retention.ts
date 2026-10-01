// Copy the existing retention queries into parameterized native SQL. No SQL is executed here.
import { readFileSync,writeFileSync,mkdirSync } from 'node:fs';
const root=new URL('../',import.meta.url);
const source=readFileSync(new URL('../../src/lib/server/integrity/history-retention.ts',import.meta.url),'utf8');
const queries=[...source.matchAll(/sql`([\s\S]*?)`/g)].map(m=>m[1]);
const history=readFileSync(new URL('../../src/lib/server/integrity/action-history.ts',import.meta.url),'utf8').match(/return sql`([\s\S]*?)`;/)![1];
const convert=(sql:string)=>sql.replaceAll('${serverId}','$1').replaceAll('${policy.maxRecords}','$2').replaceAll('${policy.maxAgeDays}','$3');
const prunable=convert(queries[2]);
const folder=new URL('sql/retention/',root);mkdirSync(folder,{recursive:true});
writeFileSync(new URL('history-index.sql',folder),convert(history).replaceAll('\r\n','\n')+'\n');
for(let index=3;index<=24;index++){
 const sql=convert(queries[index]).replaceAll('${historyIndex(serverId)}',convert(history)).replaceAll('${prunable}',prunable);
 if(sql.includes('${'))throw new Error('Unconverted parameter in '+index);
 writeFileSync(new URL(`${index}.sql`,folder),sql.replaceAll('\r\n','\n')+'\n');
}
