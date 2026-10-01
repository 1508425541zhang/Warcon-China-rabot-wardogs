import {writeFileSync} from 'node:fs';
import {liveInfantryMetrics} from '../../src/lib/server/integrity/live';
import {cashProgress} from '../../src/lib/player-progress';
import {weaponDistanceDistribution} from '../../src/lib/weapon-distance-distribution';
const now=Date.parse('2026-10-01T00:00:30Z');
const originalNow=Date.now;Date.now=()=>now;
const infantry=[];
const base=Array.from({length:21},(_,i)=>({ts:new Date('2026-10-01T00:00:00Z'),eventId:'e'+i,
 instanceId:'boot',map:'Europe',eventTime:600-i*30,cause:'Id.Item.AK74M',killerSteamId:'76561198000000001',
 victimSteamId:'765611980000000'+String(i%5+2).padStart(2,'0'),killerFaction:'BLU',victimFaction:'RED',
 headshot:false,tags:[],suicide:false,teamKill:false,factionBracketed:true,factionObservedAt:new Date('2026-10-01T00:00:00Z')}));
for(const status of [null,{map:'Europe',matchSeconds:600},{map:'Ozeti',matchSeconds:600},{map:'Kavkazi',matchSeconds:600},{map:'Europe',matchSeconds:580},{map:'Europe',matchSeconds:900}]){
 for(const change of ['valid','unknown-weapon','unknown-faction','unbracketed','old-instance','teamkill','suicide']){
  const rows=structuredClone(base);
  if(change==='unknown-weapon')rows[0].cause='Unidentified';
  if(change==='unknown-faction')(rows[0] as any).killerFaction=null;
  if(change==='unbracketed')rows[0].factionBracketed=false;
  if(change==='old-instance')rows[2].instanceId='old-boot';
  if(change==='teamkill')rows[0].teamKill=true;
  if(change==='suicide')rows[0].suicide=true;
  infantry.push({rows,status,overrides:{},now,result:Object.fromEntries(liveInfantryMetrics(rows as any,status,new Map()))});
 }
}
const distributions=[[],[10],[10,20,20,100],[1,2,5,30,1000],[0,-5,NaN,Infinity,70]].flatMap(values=>[0,123].map(current=>({values,current,result:weaponDistanceDistribution(values,current)})));
const startedAt='2026-10-01T00:00:00Z';
const samples=[{observedAt:'2026-10-01T00:00:50Z',players:[{steamId:'a',cash:60},{steamId:'b',cash:120}]},{observedAt:startedAt,players:[{steamId:'a',cash:100},{steamId:'b',cash:10}]},{observedAt:'2026-10-01T00:00:30Z',players:[{steamId:'a',cash:150},{steamId:'b',cash:90},{steamId:'c',cash:1000}]}];
const cash=['a','b','c','missing'].map(steam=>({steam,startedAt,samples,result:cashProgress(samples,steam,startedAt)}));
Date.now=originalNow;
writeFileSync(new URL('page-charts.json',import.meta.url),JSON.stringify({infantry,distributions,cash},null,2)+'\n');
