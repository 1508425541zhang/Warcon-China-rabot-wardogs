import {writeFileSync} from 'node:fs';
import {parseBoardQuery,groupCareer} from '../../src/lib/leaderboard';
const queries=[];
for(const scope of ['server','org','x']) for(const range of ['7d','30d','90d','all','x']) for(const page of ['','0','-1','1','1.5','0x10','200000','bad'])for(const max of [20,100000]){const raw=new URLSearchParams({scope,range,page,sort:page==='bad'?'DROP TABLE':'kd',dir:'asc',minMinutes:page}).toString();queries.push({raw,max,result:parseBoardQuery(new URLSearchParams(raw),max)});}
const lines=[{key:'MapA',result:'win' as const,kills:3,deaths:1},{key:'MapB',result:null,kills:5,deaths:2},{key:'MapA',result:'loss' as const,kills:1,deaths:2},{key:null,result:'draw' as const,kills:0,deaths:0}];
writeFileSync(new URL('./leaderboards.json',import.meta.url),JSON.stringify({queries,group:{lines,result:groupCareer(lines)}},null,2)+'\n');console.log(JSON.stringify({queries:queries.length}));
