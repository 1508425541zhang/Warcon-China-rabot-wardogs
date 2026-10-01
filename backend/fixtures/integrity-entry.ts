import {writeFileSync} from 'node:fs';
import {validateIntegrityRules} from '../../src/lib/server/integrity/rules';
import {DEFAULT_INTEGRITY_RULES as defaults,scoreIntegrity} from '../../src/lib/server/integrity/score';
import {classifyWeapon,countsAsInfantry} from '../../src/lib/server/integrity/weapons';
import {InfantryWindows} from '../../src/lib/server/integrity/windows';
import {generateBatchFeatures} from '../../src/lib/server/integrity/features';
import {assessDistribution,populationBucket,sampleQuality,percentilePosition} from '../../src/lib/server/integrity/statistics';
import {selectBaselines,selectWeaponBaselines} from '../../src/lib/server/integrity/baselines';
import {compareRoundPrecision,precisionDecision} from '../../src/lib/server/integrity/precision-round';
import {roundKillSeries} from '../../src/lib/server/integrity/round-change';
import {consecutiveMinutes} from '../../src/lib/server/integrity/sustained-kpm';
import {independentEpisode} from '../../src/lib/server/integrity/independence';
import {decideIntegrityAction,decideStatisticalAction,DEFAULT_ENFORCEMENT} from '../../src/lib/server/integrity/decisions';
import {integrityCapHit} from '../../src/lib/server/integrity/enforcement';
import {replayReferenceFeatures,fixtureCohorts,fixtureSummarize} from '../../src/lib/server/integrity/baseline-replay';
import {summarizeCleanCareer} from '../../src/lib/server/integrity/career-context';
const validators:any[]=[];
for(const patch of [{},{committeeKpmMinutes:2},{committeeKpmMinutes:2.5},{burstFindingMin:1},{minimumOnlineForAutoAction:0},{repeatKoWindowHours:24.5},{headshotMinKills:1.5},{quarantineThreshold:54},{oldVac:10},{mode:'enforce'},{unknown:1},{kpmBands:[]},{kpmBands:[{min:4,points:20},{min:4,points:21}]},{kpmBands:[{min:4,points:20},{min:5,points:10}]},{reportBands:[{min:1,points:0}]},{headshotMax:'3'}, {committeeKpmMinutes:null}]) {
 try {validators.push({patch,base:defaults,expected:validateIntegrityRules(patch as any),error:null});}catch(e:any){validators.push({patch,base:defaults,expected:null,error:e.message});}
}
const legacyBase={...defaults};delete (legacyBase as any).committeeKpmMinutes;
validators.push({patch:{},base:legacyBase,expected:validateIntegrityRules({},legacyBase),error:null});
let seed=12345678;const random=()=>((seed=(Math.imul(seed,1664525)+1013904223)>>>0)/4294967296);
const scores:any[]=[];
for(let i=0;i<180;i++){
 const signals={committeeMode:i%2===0,behaviorReasons:i%3===0?['burst']:[],kpm180:Math.floor(random()*36)/3,uniqueVictims:Math.floor(random()*24),previousKpm:[Math.floor(random()*10),Math.floor(random()*10)],uniqueReporters:Math.floor(random()*16),repeatHighRiskWindow:i%5===0,infantryKills:Math.floor(random()*50),headshots:Math.floor(random()*20),penetrations:Math.floor(random()*15),burstPoints:Math.floor(random()*30),vacBans:Math.floor(random()*3)-1,gameBans:Math.floor(random()*3)-1,daysSinceLastBan:[null,364,365,366][i%4],wardogsPlaytimeHours:i%3===0?null:random()*5};
 scores.push({signals,expected:scoreIntegrity(signals as any)});
}
const weapons:any[]=[];
for(const cause of [null,'Id.Item.AK74M','Id.Item.M500','Vehicle.Test','ID.VEHICLE.WEAPONEXTENSION.Test','Id.Buildable.Test','Id.Item.Unknown']) for(const tags of [[],['Suicide'],['Falling'],['RoadKill'],['VehicleExplosion']]) for(const bracketed of [null,false,true]) {
 const kill={cause,tags,suicide:false,killerSteamId:'killer',victimSteamId:'victim',killerFaction:'A',victimFaction:'B',factionBracketed:bracketed===null?undefined:bracketed,factionObservedAt:bracketed===true?'2026-09-27T00:00:00Z':null};
 const overrides=new Map([['Id.Item.Unknown','INFANTRY'],['Id.Item.AK74M','VEHICLE']] as any);
 weapons.push({kill,overrides:Object.fromEntries(overrides),classification:classifyWeapon(kill,overrides as any),infantry:countsAsInfantry(kill,overrides as any)});
}
const kill=(i:number,clock:number,patch:any={})=>({id:i,eventId:`e${i}`,instanceId:'boot',matchRow:1,eventTime:clock,map:'Europe',ts:'2026-09-27T00:00:00.000Z',killer:{steamId:'killer',faction:'A'},victim:{steamId:`v${i%7}`,faction:'B'},cause:i%3===0?'Id.Item.M4':'Id.Item.AK74M',distanceM:[null,0,5000,120][i%4],headshot:i%2===0,suicide:false,teamKill:false,tags:i%5===0?['Penetration']:[],factionBracketed:true,factionObservedAt:'2026-09-27T00:00:00.000Z',...patch});
const windows:any[]=[];
const sequences=[
 [Array.from({length:28},(_,i)=>kill(i,100+i*3)),[kill(27,181)],[kill(50,310)],[kill(51,311)],Array.from({length:18},(_,i)=>kill(60+i,320+i))],
 [[kill(1,40),kill(2,5),kill(3,25)],[kill(4,26,{map:'Kavkazi',matchRow:2})],[kill(5,27,{matchRow:1})],[kill(6,28,{map:'Kavkazi',matchRow:2})]],
 [[kill(1,30,{matchRow:null})],[kill(2,2,{map:'NorthAmerica',matchRow:null})],[kill(3,300,{map:'NorthAmerica',matchRow:null})]],
 [[kill(1,100)],[kill(2,105,{killer:{steamId:'other',faction:'A'}})],[kill(3,281,{cause:'Vehicle.Test'})],[kill(4,110)],[kill(5,282)]],
 [[kill(1,100,{factionBracketed:false}),kill(2,110,{killer:{steamId:'killer',faction:null}}),kill(3,120,{suicide:true}),kill(4,121,{teamKill:true})],[kill(5,122)], [kill(6,123,{instanceId:'new-boot'})]],
];
for(const batches of sequences){const w=new InfantryWindows();const steps:any[]=[];for(const batch of batches){const expected=generateBatchFeatures(w,'server',batch as any,new Map());steps.push({batch,expected,current:w.current('server','killer'),snapshots:w.snapshots('server',['killer','other','killer'])});for(const f of expected.findings)w.markPersisted('server',f,100+steps.length);}windows.push({steps});}
const statistics:any[]=[];
const baseline=(code:string,patch:any={})=>({id:`b-${code}`,source:'local',serverId:'server',metric:code,map:null,populationBucket:null,weaponCategory:'INFANTRY',sampleCount:200,uniquePlayers:20,uniquePlayerDays:20,effectiveSampleSize:100,modelVersion:'ensemble-server-round-v3',featureVersion:'rolling-infantry-v2',weaponMapVersion:1,generation:'g',baselineGeneration:'g',median:3,mad:1,p90:5,p95:6,p99:9,p995:10,p999:11,p9995:12,histogram:[],cdf:[[1,30],[3,50],[5,100],[8,20]],windowDays:30,calculatedAt:new Date('2026-09-27T00:00:00Z'),...patch});
for(const count of [29,30,49,50,199,200])for(const value of [0,1,3,5,8,9]) {
 const baselines=new Map(['kpm180','uniqueVictims','maxKills15s','medianKillInterval','headshotRate','penetrationRate'].map(code=>[code,baseline(code,{sampleCount:count})]));
 const values={kpm180:value,uniqueVictims:value,maxKills15s:value,medianKillInterval:value,headshotRate:value/10,penetrationRate:value/10};
 const weaponObservations=[{cause:'Id.Item.AK74M',kills:10,headshots:7,maxKillDistanceM:300}];
 const weaponBaselines=new Map([['headshotRateWeapon:Id.Item.AK74M',baseline('headshotRateWeapon',{weaponCategory:'Id.Item.AK74M'})],['maxKillDistanceWeapon:Id.Item.AK74M',baseline('maxKillDistanceWeapon',{weaponCategory:'Id.Item.AK74M',source:'external'})]]);
 statistics.push({values,baselines:Object.fromEntries(baselines),infantryKills:10,independentEpisodes:2,weaponObservations,weaponBaselines:Object.fromEntries(weaponBaselines),expected:assessDistribution(values,baselines as any,10,2,weaponObservations,weaponBaselines as any)});
}
const selectors:any[]=[];
for(const bucket of [null,'21–40'])for(const status of ['READY','STALE'])for(const age of [23,47,49])for(const server of [null,'server']) {
 const now=new Date(`2026-09-${age===49?'29':'28'}T${String(age%24).padStart(2,'0')}:00:00Z`);
 const rows=['kpm180','headshotRate','headshotRateWeapon','maxKillDistanceWeapon'].flatMap(metric=>[3,2,1].map(level=>baseline(metric,{level,source:level===3?'external':'local',map:level===1?'Europe':null,populationBucket:level===3?null:'21–40',weaponCategory:metric.endsWith('Weapon')?'Id.Item.AK74M':'INFANTRY'})));
 const state={weaponMapVersion:1,activeBaselineGeneration:'g',baselineStatus:status};
 selectors.push({rows,map:'Europe',bucket,now,state,server,expected:Object.fromEntries(selectBaselines(rows as any,'Europe',bucket as any,now,state,server??undefined)),weaponExpected:Object.fromEntries(selectWeaponBaselines(rows as any,'Europe',bucket as any,now,state,server??undefined))});
}
const precision:any[]=[];
for(const kills of [4,5,10])for(const heads of [0,3,4,5,7,9,10]) {
 const rows=[{steamId:'own',category:'automatic',kills,headshots:heads},{steamId:'own',category:'sniper',kills,headshots:heads},{steamId:'own',category:'shotgun',kills,headshots:heads},{steamId:'peer',category:'automatic',kills:10,headshots:3},{steamId:'peer',category:'automatic',kills:10,headshots:3},{steamId:'peer2',category:'automatic',kills:5,headshots:4}];
 const expected=compareRoundPrecision(rows,'own');precision.push({rows,id:'own',expected,decisions:expected.map(precisionDecision)});
}
const series:any[]=[];const minutes:any[]=[];
for(const clock of [0,120,180,299,300,700,90000]){const clocks=Array.from({length:50},(_,i)=>i*10);series.push({clocks,from:0,clock,expected:roundKillSeries(clocks,0,clock)});for(const required of [2,3])minutes.push({clocks,clock,minutes:required,threshold:4,expected:consecutiveMinutes(clocks,clock,required,4)});}
const episodes:any[]=[];
for(const roundId of [null,'boot:match:1','boot:match:2'])for(const gap of [59,60,61])for(const eventIds of [[],['old'],['new'],['']]){const saved={eventIds,roundId,clockTo:100,observedAt:new Date('2026-09-27T00:00:00Z')};const current={eventIds:['new'],roundId:'boot:match:1',clockFrom:100+gap,observedAt:new Date(saved.observedAt.getTime()+gap*1000)};episodes.push({saved,current,separation:60,expected:independentEpisode(saved,current,60)});}
const decisions:any[]=[];
for(const score of [40,54,64,100])for(const kpm of [4,8])for(const active of [false,true])for(const online of [19,20]){
 const input={score:{score,currentBehaviorAnomaly:active},finding:{kpm180:kpm,burstPoints:12,reasons:['kpm','burst']},confidence:'B',feedHealthy:true,playerOnline:true,onlinePlayers:online,identityReliable:true,priorIndependentWindow:true,previousActions:[],rules:defaults,settings:{...DEFAULT_ENFORCEMENT,autoKickEnabled:true,autoQuarantine24hEnabled:true,autoQuarantine7dEnabled:true}};
 decisions.push({input,statistical:false,expected:decideIntegrityAction(input as any)});
}
const caps:any[]=[];for(const org of [0,9,10])for(const server of [0,1,2])for(const online of [1,20,30]){const settings=DEFAULT_ENFORCEMENT;caps.push({org,server,online,settings,expected:integrityCapHit(org,server,online,settings)});}
const metadata=[null,0,1,20,21,40,41,60,61,80,81,1.5,-1].map(count=>({count,bucket:populationBucket(count),quality:sampleQuality(count??0)}));
const replays:any[]=[];
for(const rawRound of [false,true]){
 const rows:any[]=[];
 for(let day=0;day<7;day++)for(let i=0;i<70;i++)rows.push({source:'local',server_id:i%5===0?'other':'server',event_id:`事件😀-${day}-${i}`,at:new Date(Date.UTC(2026,8,20+day,0,0,i*3)),instance_id:'boot',match_id:`round-${day}`,match_row:rawRound?String(9+day):9+day,event_time:i*3,map:day%2===0?'Europe':'Kavkazi',killer_steam_id:`p${i%3}`,victim_steam_id:`v${i%8}`,killer_faction:'A',victim_faction:'B',cause:i%5===0?'Id.Item.M4':'Id.Item.AK74M',distance_m:i%3===0?120:null,headshot:i%3===0,penetration:i%4===0,player_count:day%3===0?null:25});
 rows.push({...rows.at(-1),match_row:'9007199254740992',event_id:'invalid'}, {...rows.at(-1),match_row:'0',event_id:'zero'});
 const overrides={};const expected=replayReferenceFeatures(rows,new Map());const groups=fixtureCohorts(expected);
 replays.push({rows,overrides,expected,cohorts:groups,summaries:groups.map(fixtureSummarize)});
}
const careers:any[]=[];
for(const days of [0,1,5,15]){const rows:any[]=[];for(let day=0;day<days;day++)for(let i=0;i<8;i++)rows.push({roundId:`round-${day%3}`,observedAt:new Date(Date.UTC(2026,8,1+day,0,0,i*60)),kpm180:(day+i)%6,headshotRate:i%3===0?null:i/10,maxKills15s:i});rows.reverse();careers.push({rows,expected:summarizeCleanCareer(rows)});}
writeFileSync('backend/fixtures/integrity.json',JSON.stringify({defaults,validators,scores,weapons,windows,statistics,selectors,precision,series,minutes,episodes,decisions,caps,metadata,replays,careers},null,2)+'\n');
console.log(JSON.stringify({validators:validators.length,scores:scores.length,weapons:weapons.length,windowSequences:windows.length,statistics:statistics.length,selectors:selectors.length,precision:precision.length,decisions:decisions.length}));
