import { ACTIONS } from '../../src/lib/server/actions';
const STEAM='76561198388453389';
const text='[RCON]\r\nPassword="fixture-password"\r\n;Token=fixture-token\r\n[/Script/WDGame.WDGameSession]\r\nMaxPlayers=100\r\n!DefaultReservedPlayerIds=ClearArray\r\n.DefaultReservedPlayerIds=76561198000000001\r\n';
const maps=[{id:'map-one',displayName:'One'},{id:'map-two',displayName:'Two'}];
const rotation={enabled:true,mode:'ORDERED',entries:[{map:'map-one',experiences:['CTF'],lighting:'Day',zoneAlternator:'None',status:'now',denied:false},{map:'map-two',experiences:['CTF'],lighting:'Day',zoneAlternator:'None',status:'next',denied:false}]};
const replies:Record<string,unknown>={
 '/v1/capabilities':{routes:['PATCH /v1/players/{steamId}','PUT /v1/config','POST /v1/reserved-slots','POST /v1/rotation/entries','POST /v1/rotation/entries/:index/move','POST /v1/rotation/save','PATCH /v1/settings','GET /v1/server-id'],config:{writable:true}},
 '/v1/status':{serverName:'测试游戏服',map:'map-one',experiences:['CTF'],lighting:'Day',alternator:'A',scoreTick:{current:24,min:1,max:600},scoreCap:200,matchSeconds:80,players:{current:20,max:100},factionScores:[{name:'red',score:100,colorHex:'ff0000'}],rotation:{nowIndex:0,nextIndex:1}},
 '/v1/health':{status:'running',uptimeSeconds:123},'/v1/server-id':{serverId:'join-test'},
 '/v1/players':{players:[{name:'测试玩家',steamId:STEAM,faction:'red',kills:9,deaths:2,cash:999,pingMs:24}]},
 '/v1/catalog/maps':{maps},'/v1/catalog/lightings':{lightings:[{id:'Day',displayName:'白天'}]},'/v1/catalog/experiences':{experiences:[{id:'CTF',displayName:'Flag'}]},
 '/v1/catalog/maps/map-one/experiences':{experiences:['CTF','other']},'/v1/catalog/maps/map-one/alternators':{alternators:[{tag:'A',displayName:'Alpha'}]},
 '/v1/rotation':rotation,'/v1/bans':{bans:[{steamId:STEAM,bannedAtUtc:'0001-01-01T00:00:00Z',bannedBy:'config',reason:'test'}]},'/v1/reserved-slots':{reservedSlots:[STEAM]},
 '/v1/sponsor':{imageUrl:'https://example.test/cover.png'},'/v1/audit?limit=40':{entries:[{timestampUtc:'2026-10-01T00:00:00Z',peer:'127.0.0.1:123',sessionId:'test-session',event:'AUTH_OK',detail:'test'}]},
 '/v1/config':{text,revision:'r1',writable:true,sections:[{name:'RCON'}],warnings:['Token=fixture-token']}
};
class Mock {
 calls:any[]=[];
 async raw(method:string,path:string,body?:string,headers:Record<string,string>={}){
  const data=method==='GET'?replies[path]??{message:'ok'}:{message:'ok'};
  const response={status:200,statusText:'OK',headers:{'content-type':'application/json',etag:'"r1"',location:'https://private.test',via:'private'},text:JSON.stringify(data)};
  this.calls.push({method,path,body:body??null,headers,response});return response;
 }
 async json(method:string,path:string,body?:unknown){const r=await this.raw(method,path,body===undefined?undefined:JSON.stringify(body),body===undefined?{}:{'Content-Type':'application/json'});return JSON.parse(r.text);}
 async configCall(method:string,path:string,body:string,revision?:string){
  const data={ok:true,revision:'r2',changed:['Password="fixture-password"'],warnings:['fixture-token'],outcomes:[],errors:[]};
  const headers:any={'Content-Type':'text/plain'};if(revision)headers['If-Match']=`"${revision}"`;
  this.calls.push({method,path,body,headers,response:{status:200,statusText:'OK',headers:{etag:'"r2"'},text:JSON.stringify(data)}});return {status:200,body:data,etag:'r2'};
 }
}
const cases:any[]=[];
for (const name of Object.keys(ACTIONS)){
 const params:any={steamId:STEAM,message:'测试消息',reason:'test',faction:'blue',map:'map-one',lighting:'Day',experiences:['CTF'],index:1,direction:'up',from:1,to:0,limit:40,document:1,text:text.replace(/"fixture-password"/g,'(hidden)').replace('Token=fixture-token','Token=(hidden)'),revision:'r1',rotationEnabled:'on',rotationMode:'random',scoreTick:27,method:'GET',path:'/v1/health',raw:true};
 const client=new Mock();const result=await ACTIONS[name].run(client as any,params);cases.push({name,params,calls:client.calls,result});
}
await Bun.write('backend/fixtures/actions.json',JSON.stringify(cases,null,2)+'\n');
console.log(`Generated ${cases.length} original RCON action contracts`);
