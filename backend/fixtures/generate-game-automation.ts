import {writeFileSync} from 'node:fs';
import {factionLockDecision,originalFactionLeading} from '../../src/lib/faction-lock-policy';
import {planSkillBalance} from '../../src/lib/skill-balance-policy';
import {quotaInput,quotaTeams,planQuota} from '../../src/lib/faction-quota-policy';
import {restrictedCause,restrictionStage} from '../../src/lib/weapon-restriction-policy';
import {parseIni,getScalar,setScalarInText} from '../../src/lib/config-doc';
const factions=['Blue','Red','Green'];const decisions=[];for(const from of [null,'','Blue','Red','Observer'])for(const to of [null,'Blue','Red'])for(const authorized of [false,true])for(const transition of [false,true])for(const full of [null,false,true])for(const leading of [null,false,true]){const input={from,to,teams:factions,authorized,transition,full,leading};decisions.push({input,output:factionLockDecision(input)})}
const scores=[{name:'Blue',score:150,colorHex:'#0000ff'},{name:'Red',score:60,colorHex:'ff0000'},{name:'Green',score:30,colorHex:'#00ff00'}];
const leading=[...factions,'Unknown'].map(faction=>({scores,faction,output:originalFactionLeading(scores,faction)}));
const players=Array.from({length:9},(_,i)=>({steamId:String(76561198000000000n+BigInt(i)),name:'P'+i,faction:factions[Math.floor(i/3)],kills:20-i,deaths:5,cash:100,kpm:8-i/2,kd:4-i/5}));
const balance=[];for(const lead of [39,40,90,100])for(const count of [4,9]){const status={scores};balance.push({status,players:players.slice(0,count),lead,output:planSkillBalance(status as any,players.slice(0,count) as any,lead)})}
const quota=[];for(const limits of [{blue:50,red:50,green:0},{blue:40,red:30,green:30},{blue:2,red:2,green:0}])for(const n of [0,1,3,7,11]){const roster=Array.from({length:n},(_,i)=>({...players[i%9],steamId:String(76561198000000000n+BigInt(i)),faction:i<n-1?'Blue':'Red',kills:n-i}));const teams=quotaTeams(scores)!;const blocked=[players[0].steamId];quota.push({players:roster,teams,limits,blocked,output:planQuota(roster as any,teams,limits,new Set(blocked))})}
const teamCases=[scores,scores.slice(0,2),scores.map(s=>({...s,colorHex:'#888888'})),scores.map(s=>({...s,name:'same'}))].map(scores=>({scores,output:quotaTeams(scores)}));
const quotaConfigs=[{revision:'empty',enabled:true,limits:{blue:50,red:50,green:0}},{revision:'x',enabled:false,limits:{blue:100,red:0,green:0}},{revision:'x',enabled:true,limits:{blue:50,red:50,green:20}}].map(input=>{const p=quotaInput.safeParse(input);return p.success?{input,output:p.data}:{input,error:true}});
const causes=[null,'','Id.Item.AK74M','id.item.Grenade','Id.Buildable.Turret','Vehicle.Tank','Id.Vehicle.Tank','Other'];const weapons=causes.flatMap(cause=>[['items'],['vehicles'],['buildables'],[]].map(groups=>({cause,rule:{groups,causes:['Other']},output:restrictedCause(cause,['Other'],groups)})));
const stages=[[-1,60,null],[0,60,null],[65,60,null],[66,60,null],[100,100,50],[110,110,50]].map(([event,now,last])=>({event,now,last,output:restrictionStage(event!,now!,last)}));
const section='/Script/WDGame.WDGameStateSession',key='bLockOverpopulatedTeamsConfig';const scalar=['',`[${section}]\r\n  ${key}=true\r\n; comment\r\nPassword=keep\r\n`,`[Other]\nValue=1\n`,`[${section}]\n+${key}=true\n${key}=false\n`].map(text=>({text,section,key,value:'false',before:getScalar(parseIni(text),section,key),after:setScalarInText(text,section,key,'false')}));
writeFileSync(new URL('./game-automation.json',import.meta.url),JSON.stringify({decisions,leading,balance,quota,teamCases,quotaConfigs,weapons,stages,scalar},null,2)+'\n');
