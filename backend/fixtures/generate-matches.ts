import { awardsFor } from '../../src/lib/matches';
import { writeFileSync } from 'node:fs';
const line=(id:string,kills:number,deaths:number,more={})=>({steamId:id,name:id,faction:'Red',kills,deaths,seconds:1800,cashDelta:1234567,headshots:1,teamKills:0,suicides:0,vehicleKills:0,longestM:120.5,killStreak:3,deathStreak:0,result:'win',...more});
const cases=[{lines:[],duration:1800},{lines:[line('early',10,3)],duration:1199},{lines:[line('winner',17,8),line('tie',17,8)],duration:1200},{lines:[line('binary-round',107,40)],duration:1800},{lines:[line('no-deaths',10,0)],duration:1800},{lines:[line('no-kills',0,0,{cashDelta:-1,longestM:null,killStreak:2})],duration:1800},{lines:[line('low',9,1),line('winner',10,3,{cashDelta:123,longestM:90})],duration:1800}];
writeFileSync(new URL('./matches.json',import.meta.url),JSON.stringify(cases.map(c=>({...c,expected:awardsFor(c.lines as any,c.duration)})),null,2)+'\n');
console.log(`${cases.length} match award cases`);
