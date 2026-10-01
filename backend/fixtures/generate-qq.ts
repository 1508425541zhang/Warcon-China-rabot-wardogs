import {writeFileSync} from 'node:fs';
import {qqSignature,oneBotMessage,parseCommand,friendlyTargets} from '../../src/lib/server/qq/protocol';
import {officialMessage,officialValidation,verifyOfficial} from '../../src/lib/server/qq/official';
import {battleMessage,factionBlock} from '../../src/lib/server/qq/battle';
const now=1790812800123,self='123456',secret='fixture-official-qq-secret',time=String(Math.floor(now/1000));
const base={time:Math.floor(now/1000),self_id:self,post_type:'message',message_type:'group',sub_type:'normal',group_id:567890,user_id:876543,message_id:-123,message:'/积分'};
const messages=['/积分',' [CQ:at,qq=123456] /投票 2','/友方广播 &#91;测试&#93; &amp;','not command','/ 空格','！！帮助','/x[CQ:image,file=x]','/'+'😀'.repeat(1000),[{type:'at',data:{qq:self}},{type:'text',data:{text:'/战绩 玩家'}}],[{type:'image',data:{file:'x'}}]];
const onebot=messages.map(message=>{const input={...base,message};return{input,expected:oneBotMessage(input,self,now)}});
for(const changes of [{anonymous:{}},{user_id:self},{self_id:'999999'},{time:Math.floor(now/1000)-301},{message_id:'-99999999999999999999'},{group_id:'01234'},{sub_type:'anonymous'}]){const input={...base,...changes};onebot.push({input,expected:oneBotMessage(input,self,now)})}
const officialBase={op:0,t:'GROUP_AT_MESSAGE_CREATE',d:{id:'id:消息_1',group_openid:'groupOpenID0123456789',author:{member_openid:'memberOpenID0123456789'},timestamp:new Date(now).toISOString(),content:'<@!123456> /积分'}};
const official=[officialBase,...[{content:'plain message'},{attachments:[{}]},{timestamp:new Date(now-300001).toISOString()},{author:{member_openid:'memberOpenID0123456789',bot:true}},{content:' /战绩 张三'}].map(d=>({...officialBase,d:{...officialBase.d,...d}}))].map(input=>({input,expected:officialMessage(input,self,now)}));
const commands=[' /积分  ','！投票   2','<@!123456> /战绩 小王','!!帮助','/',' /友方广播 a\nb'];
const roster=[{steamId:'76561198000000001',name:'张三',faction:'RED'},{steamId:'76561198000000002',name:'李四',faction:'BLU'},{steamId:'76561198000000003',name:'王五',faction:'GRN'}];
const status={serverName:'测试服务器',map:'Europe',matchSeconds:123,scoreCap:100,scores:[{name:'RED',score:70,colorHex:'#ff0000'},{name:'BLU',score:30,colorHex:'#0000ff'},{name:'GRN',score:20,colorHex:'#00ff00'}]};
const battles=[{status,roster,page:1},{status:{...status,scoreCap:null,matchSeconds:null},roster:[],page:1},{status,roster:Array.from({length:55},(_,i)=>({...roster[i%3],steamId:String(76561198000000001n+BigInt(i)),name:'玩家 '+i})),page:2}].map(c=>({...c,expected:battleMessage(c.status,c.roster,c.page)}));
const signed=officialValidation(secret,JSON.stringify(officialBase),time);const signature=officialValidation(secret,'challenge',time);
writeFileSync(new URL('qq.json',import.meta.url),JSON.stringify({now,self,secret,time,onebot,official,commands:commands.map(content=>({content,expected:parseCommand(content)})),battles,blocks:[['custom','#fc0100'],['custom','#0304fe'],['GRN',''],['other','#444444']].map(([name,hex])=>({name,hex,expected:factionBlock(name,hex)})),hmac:{body:JSON.stringify(base),signature:qqSignature(secret,JSON.stringify(base))},signed:{body:JSON.stringify(officialBase),signature:signed.signature,expected:verifyOfficial(secret,new Headers({'x-signature-ed25519':signed.signature,'x-signature-timestamp':time}),Buffer.from(JSON.stringify(officialBase)),now)},validation:signature},null,2)+'\n');
