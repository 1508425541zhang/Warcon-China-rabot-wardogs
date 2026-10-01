import {normalizeStatus} from '../../src/lib/server/runtime-normalizers';
const variants:any[]=[null,{},[],{serverName:'s',map:'m'},'bad'];
const nums:any[]=[null,0,1,-1,1.2,'0','-2','1.5','01',' 1','1e3','1.','0x10',true,{},[],1e308];
for(const value of nums){variants.push({serverName:'Server',map:'Map',playerCount:value,maxPlayers:value,matchSeconds:value,scoreTick:value,scoreCap:value,rotationNow:value,scores:[{name:'Red',score:value,colorHex:'#a1B2c3'},{name:'Green',score:1,colorHex:'bad'},{name:'',score:3}]});}
for(const scores of [null,[],{},[null,1,{}],[{name:'Red',score:'10',colorHex:'#123456'},{name:'Red',score:2,colorHex:'#ffffff'}]]){variants.push({serverName:'服务器',map:'测试',experiences:['a',null,3,'b'],scores,lighting:3,alternator:[]});}
await Bun.write(new URL('./live.json',import.meta.url),JSON.stringify(variants.map(input=>({input,expected:normalizeStatus(input)})),null,2)+'\n');
