import {writeFileSync} from 'node:fs';
import {parseBatch} from '../../src/lib/server/feed-core';
const event={type:'killed',eventId:'kill-1',matchId:'round',eventTime:10,mapName:'Zestafona',killerSteamId:'76561198388453389',killerName:'玩家',victimSteamId:'76561198000000001',victimName:'Victim',cause:'Id.Item.AK74M',contextTags:['Meta.PlayerKillFlag.Player.Headshot','Meta.Progression.Context.Player.KillContext.Penetration','Local.Kill','Local.Death'],distance:15000};
const inputs:any[]=[{serverId:'boot',serverName:'Test',events:[]},{serverId:'boot',events:[event]},{events:[{...event,type:'KO'},event]},{events:[{...event,killerSteamId:'broken'}]},{events:[{...event,eventTime:'10'}]},{events:[{...event,killerSteamId:event.victimSteamId}]},{events:[{...event,contextTags:['Local.Kill','Headshot','Headshot','Suicide','RoadKill']}]},{events:[{...event,mapName:'ozeti'}]}];
for(const distance of [null,'100',0,-1,1,0.49,499999,500000,1e100])inputs.push({events:[{...event,distance}]});
const cases=inputs.map(input=>({input,expected:parseBatch(input)}));
writeFileSync(new URL('./feed.json',import.meta.url),JSON.stringify(cases,null,2)+'\n');console.log(JSON.stringify({feedCases:cases.length}));
