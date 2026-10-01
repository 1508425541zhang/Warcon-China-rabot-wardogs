import {writeFileSync} from 'node:fs';
import {planSync,desiredOf,isAlreadyApplied,isGone,isUnreachable} from '../../src/lib/server/lists-plan';
import {renderBanMessage} from '../../src/lib/ban-message';
const now=new Date('2026-09-27T06:34:10.000Z');
const plans=[];
for(const present of [false,true]) for(const wanted of [false,true]) for(const managed of [false,true]) for(const state of ['applied','failed'] as const) for(const age of [null,0,299999,300000,600000]) for(const source of ['org','server']) {
 const input={now,retryAfterMs:300000,desired:{bans:[{steamId:'76561198000000009',listId:'org',reason:'ban'}],reserved:wanted?[{steamId:'76561198000000001',listId:'org',member:false}]:[]},observed:{reserved:present?['76561198000000001']:[]},state:managed?[{kind:'reserve' as const,steamId:'76561198000000001',sourceListId:source,state,error:'',attemptedAt:age===null?null:new Date(+now-age)}]:[]};
 plans.push({input,result:planSync(input)});
}
const rows=[{kind:'reserve' as const,steamId:'1',reason:'own',listId:'server',serverId:'s'},{kind:'reserve' as const,steamId:'1',reason:'org',listId:'org',serverId:null},{kind:'ban' as const,steamId:'1',reason:'ban',listId:'orgban',serverId:null}];
const failures=[];
for(const status of [200,400,404,405,409,412,429,500,502,503]) for(const code of ['', 'already','not_found','no_route','revision_conflict','unreachable','rate_limited']) for(const message of ['Already exists','already_applied','not already','ready']) {const input={status,code,message};failures.push({input,already:isAlreadyApplied(input),gone:isGone(input),unreachable:isUnreachable(input)});}
const messages=[];
for(const minutes of [null,-1,0,1,45,59.5,60,61,1440,2160,10080]) for(const template of ['', '{REASON} | {duration} | {expires} | {banned} | {uid} | {admin}', '{reason} | {unknown}', '· : {reason} ; -']) {const facts={entryId:'abcdef-12345',reason:minutes===0?'':'测试原因',addedByName:'Admin',addedAt:now,expiresAt:minutes===null?null:new Date(+now+minutes*60000)};messages.push({template,facts,result:renderBanMessage(template,facts)});}
writeFileSync(new URL('./lists.json',import.meta.url),JSON.stringify({plans,desired:{rows,result:desiredOf(rows)},failures,messages},null,2)+'\n');
console.log(JSON.stringify({plans:plans.length,failures:failures.length,messages:messages.length}));
