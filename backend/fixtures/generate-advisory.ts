import {writeFileSync} from 'node:fs';
import {assessRisk,normaliseName,namesResemble} from '../../src/lib/server/risk';
import {defaultGroupConfig,groupConfigSchema,namePrefix,prefixSimilarity,detectGroups} from '../../src/lib/group-control-policy';
import {awardWinners} from '../../src/lib/match-awards-policy';
const now=new Date('2026-09-30T12:00:00Z');
const profile={public:true,accountCreatedAt:new Date('2020-01-01T00:00:00Z'),vacBans:0,gameBans:0,daysSinceLastBan:null,communityBanned:false,economyBan:'none'};
const inputs:any[]=[null,{...profile},{...profile,public:false,friendsState:'private'},{...profile,vacBans:1,daysSinceLastBan:10},{...profile,vacBans:9,gameBans:3,daysSinceLastBan:2000},{...profile,accountCreatedAt:now},{...profile,friendsState:'partial',friendsTotal:100,friendsChecked:50,bannedFriends:3},{...profile,error:'not_available'}];
const risk=inputs.map(p=>{const input={profile:p,steamEnabled:true,watched:null,bannedOn:[],resembles:[],now};return {input,output:assessRisk(input)}});
for(const perf of [{matches:25,wins:20,losses:4,draws:1,kills:100,deaths:20,feedKills:50,headshots:30},{matches:19,wins:19,losses:0,draws:0,kills:99,deaths:19,feedKills:49,headshots:40}]){const input={profile:null,steamEnabled:false,watched:{reason:'review'},bannedOn:[{serverName:'Other',reason:'confirmed'}],resembles:[{name:'Name',steamId:'123',serverName:'Other'}],performance:perf,now};risk.push({input,output:assessRisk(input)} as any)}
const names=['José','Ｓｔｅａｍ','[a] 12','[CLAN] Alice','ＣＬＡＮ Bob','clan2','clon3','abc','abcdefghij','abhdefghij','a','玩家组队','Русский','😀😀'];
const prefixes=names.map(name=>({name,length:4,output:namePrefix(name,4),normal:normaliseName(name)}));
const comparisons=names.flatMap(a=>names.map(b=>({a,b,similarity:prefixSimilarity(a,b),resembles:namesResemble(a,b)})));
const groups=[];for(const list of [['[CLAN] Alice','ＣＬＡＮ Bob','clan2','clon3','other'],['aaaa','aaab','aabb','bbbb'],['玩家组队甲','玩家组队乙','玩家组队丙']]){const players=list.map((name,i)=>({name,steamId:String(76561198000000000n+BigInt(i)),faction:'Blue'}));for(const threshold of [0,70,75,99]){const cfg={...defaultGroupConfig,mode:'auto',minPlayers:2,similarityPercent:threshold};groups.push({players,cfg,factions:['Blue','Red','Blue'],output:detectGroups(players as any,groupConfigSchema.parse(cfg),['Blue','Red','Blue'])})}}
const awardSets=[['😀','🦊','a'],['𠀀','😀','b']].map(names=>{const lines=names.map(name=>({name,kills:8,deaths:2,seconds:60,awardSeed:'tie😀'}));return {lines,output:awardWinners(lines as any)}});
writeFileSync(new URL('./advisory.json',import.meta.url),JSON.stringify({risk,prefixes,comparisons,groups,awardSets},null,2)+'\n');
