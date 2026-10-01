import {shortWindowConsensus,SHORT_KICK,SHORT_WARNING,shortRiskDecision} from '../../src/lib/short-risk-policy';
const start=Date.parse('2026-09-01T00:00:00Z');const iso=(n:number)=>new Date(n).toISOString();
const cases=[];
for(const length of [0,4,5,6])for(const step of [7000,8000,10000,20000,21000])for(const score of [SHORT_WARNING,SHORT_KICK,SHORT_KICK+1e-6,.9]){
 const windows=Array.from({length},(_,i)=>({at:iso(start+i*step),score}));const end=windows.at(-1)?.at??iso(start);cases.push({windows,end,expected:shortWindowConsensus(windows,end)});
}
await Bun.write('backend/fixtures/short-policy.json',JSON.stringify({kick:SHORT_KICK,warning:SHORT_WARNING,cases,decisions:[0,SHORT_WARNING,SHORT_WARNING+1e-6,SHORT_KICK,SHORT_KICK+1e-6,1].map(score=>({score,expected:shortRiskDecision(score)}))})+'\n');
