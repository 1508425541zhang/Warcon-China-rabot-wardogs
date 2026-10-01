import {diffPresence,followPlayer} from '../../src/lib/server/sessions';
import {tallyLook,tallyRow,closeTallies,feedRecord} from '../../src/lib/server/match-players';
import {matchBoundary} from '../../src/lib/server/trigger-rules';
import {phaseOffset,nextDue} from '../../src/lib/server/poller-schedule';
const sort=(a:any[])=>a.sort((a,b)=>a.steamId.localeCompare(b.steamId));
const p=(steamId:string,kills=1,deaths=1,cash=20,faction:any='Red')=>({steamId,name:steamId,kills,deaths,cash,faction,ping:null});
const session=(steamId:string,more={})=>({id:1,steamId,name:steamId,faction:'Red',kills:3,deaths:2,cash:100,game:{kills:3,deaths:2,cash:100},seedMs:0,pendingSeedMs:0,joinedAt:1000,lastSeen:2000,writtenAt:2000,firstVisit:false,lastFaction:'Red',team:'Red',...more});
const presence=[];
for(const now of [2000,3000,62000,62001,70000])for(const faction of [null,'','White','Red','Blue'])for(const name of ['a','renamed']){
 const open=[session('a'),session('b',{lastSeen:1000})];const players=[{...p('a',5,3,120,faction),name},p('c'),p('c')];const teams=['Red','Blue','Green'];
 const diff=diffPresence({open:new Map(open.map(s=>[s.steamId,s])),loaded:true,heartbeatAt:0} as any,players,now,60000,2500,teams);
 presence.push({open,players,now,grace:60000,previous:2500,teams,expected:{...diff,stayed:diff.stayed.map(x=>x.player),left:sort(diff.left)}});
}
const follow=[];
for(const game of [null,{kills:3,deaths:2,cash:100}])for(const kills of [0,3,6])for(const deaths of [0,2,5])for(const cash of [0,50,120])for(const faction of [null,'White','Blue']){
 const before=session('a',{game});const player=p('a',kills,deaths,cash,faction);const after=structuredClone(before);const teams=['Red','Blue'];followPlayer(after as any,player,3000,teams);follow.push({before,player,now:3000,teams,expected:after});
}
const tallies=[];
for(const reset of [false,true])for(const cut of [1000,3000,4000])for(const faction of [null,'White','Blue']){
 const t=new Map();const steps=[{players:[p('a',3,1,200),p('b',1,0,50)],now:1000,gap:1000,stayed:[]},{players:[p('a',5,2,1000,faction)],now:3000,gap:2000,stayed:['a']},{players:[p('a',reset?1:7,reset?0:2,reset?50:1200,faction)],now:4000,gap:1000,stayed:['a']}];
 for(const step of steps)tallyLook(t as any,step.players,{...step,gapMs:step.gap,stayed:new Set(step.stayed),teams:['Red','Blue']});
 const closed=closeTallies(t as any,cut);tallies.push({steps,teams:['Red','Blue'],cut,expected:{tallies:sort([...t.values()]),rows:sort([...t.values()].map(tallyRow as any)),closed:sort(closed.rows),carried:sort([...closed.carried.values()])}});
}
const boundaries=[];
const look=(map:string,n:number,seconds:any)=>({map,scores:[{name:'Red',score:n},{name:'Blue',score:n===7?n:1},{name:'Green',score:0}],matchSeconds:seconds});
for(const previous of [null,look('A',0,null),look('A',5,1800),look('A',7,100)])for(const next of [look('A',8,null),look('B',9,0),look('A',0,0),look('A',7,80),look('A',7,60)])boundaries.push({previous,next,expected:matchBoundary(previous,next)});
const phases=['server','服务器','玩家😀','', 'x-y_100'].flatMap(id=>[0,1,30000,1000].map(interval=>({id,interval,expected:phaseOffset(id,interval)})));
const due=[0,1000,2000,3000].flatMap(due=>[500,1000].flatMap(interval=>[1000,2000].map(now=>({due,interval,now,expected:nextDue(due,interval,now)}))));
const feed = [];
for(const suicide of [false,true])for(const teamKill of [false,true])for(const cause of ['weapon.ak','Vehicle.tank','ID.Vehicle.WeaponExtension.gun']){
 const kills=[{killerSteamId:'a',victimSteamId:'b',headshot:true,suicide:false,teamKill:false,cause,distanceM:12},{killerSteamId:'a',victimSteamId:'c',headshot:true,suicide,teamKill,cause,distanceM:24},{killerSteamId:'b',victimSteamId:'a',headshot:false,suicide:false,teamKill:false,cause,distanceM:null},{killerSteamId:null,victimSteamId:'a',headshot:false,suicide:true,teamKill:false,cause,distanceM:0}];
 feed.push({kills,expected:Object.fromEntries(feedRecord(kills))});
}
await Bun.write('backend/fixtures/observation.json',JSON.stringify({presence,follow,tallies,boundaries,phases,due,feed},null,2)+'\n');
