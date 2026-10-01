import { writeFileSync } from 'node:fs';
import { validNumericLimits, numericBreaches, defaultNumericLimits } from '../../src/lib/numeric-limit-policy';
import { summarizePlaytime } from '../../src/lib/server/playtime-distribution';
const limits:any[]=[];
for(const enabled of [false,true])for(const kpm of [null,-1,0,2,1e9])for(const windowSeconds of [59,60,180,900,901]) {
 const input={...defaultNumericLimits,enabled,kpm,windowSeconds};limits.push({input,valid:Boolean(validNumericLimits(input))});
}
const rule={...defaultNumericLimits,enabled:true,kpm:4,kd:3,cash:1000};
const standard=()=>[0,60000,120000,180000].map((at,i)=>({at,kills:i*6,deaths:i,cash:i*2000}));
const pointsets:any[]=[[],standard(),standard().slice(0,1),standard().slice(0,3),standard().map((p,i)=>({...p,kills:i*2})),standard().map((p,i)=>({...p,cash:-i*10})),standard().map((p,i)=>({...p,kills:i===3?1:p.kills})),standard().map((p,i)=>({...p,at:i===2?140000:p.at})),standard().map((p,i)=>({...p,deaths:0}))];
const breaches=pointsets.map(points=>({rule,points,expected:numericBreaches(rule,points)}));
const playtime=[];
for(const groups of [[],[{minutes:null,state:'unavailable',count:2}],[{minutes:0,state:'known',count:1},{minutes:600,state:'known',count:7},{minutes:12000,state:'known',count:2},{minutes:null,state:'error',count:1},{minutes:600,state:'pending',count:3}],[{minutes:2147483647,state:'known',count:1},{minutes:30,state:'known',count:9}]]) {
 playtime.push({groups,expected:summarizePlaytime(groups,true)});
}
writeFileSync(new URL('./analytics.json',import.meta.url),JSON.stringify({limits,breaches,playtime},null,2)+'\n');
console.log(JSON.stringify({limits:limits.length,breaches:breaches.length,playtime:playtime.length}));
