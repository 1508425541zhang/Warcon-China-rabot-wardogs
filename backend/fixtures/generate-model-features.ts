import { buildExpanded, normalizeRows } from '../../services/integrity-model-bun/expanded-features';
import { writeFileSync } from 'node:fs';
const weapons=['Id.Item.AK74M','Id.Item.SVDM','Id.Item.M500','Id.Vehicle.Tank','UNKNOWN'];
const classes=['automatic','sniper','shotgun','other_infantry','vehicle','fixed_weapon','unknown'];
const features:string[]=[];
for(const w of [60,120]) for(const n of ['small_arm_kills','kills','deaths','headshot_rate','penetration_rate','unique_victims','max_kills_15s','median_interval_s','distance_ratio_p90','headshot_samples','distance_samples'])features.push(`${n}_${w}s`);
features.push('kill_rate_change','dominant_weapon_fraction_120s','distance_mean_m_120s','distance_max_m_120s','all_headshot_rate_120s','all_headshot_samples_120s','raw_event_fraction_120s','suicides_120s','teamkills_120s','kill_clock_spread_120s','player_kills','player_delta_kills','player_cash','player_delta_cash','player_pingMs','player_delta_pingMs','roster_size','own_faction_score','leading_faction_score','faction_score_gap','round_elapsed_s','feed_age_s','roster_age_s','utc_hour_sin','utc_hour_cos','status:metrics.frameMs','tag_120s:Penetration','tag_120s:Headshot','faction:Red','faction:Blue','faction:UNKNOWN','map:TestMap','map:UNKNOWN','lighting:Day','lighting:UNKNOWN','experience:Infantry');
for(const w of weapons)for(const n of ['kills_120s','headshots_120s','headshot_rate_120s','mean_distance_ratio_120s'])features.push(`weapon:${w}:${n}`);
for(const c of classes)for(const n of ['kills_120s','headshots_observed_120s','headshot_samples_120s','headshot_rate_120s','max_weapon_mean_distance_ratio_120s'])features.push(`class:${c}:${n}`);
const distance=Object.fromEntries(weapons.map((w,i)=>[w,{p95_m:100+i*75}]));
const contract={features,feature_groups:features.map(()=> 'combat'),window_steps:60,center:features.map((_,i)=>i%3-.5),scale:features.map((_,i)=>1+i%5),base:{features,weapon_vocabulary:weapons,distance_baseline:distance},distance_baseline:distance,weapon_class_by_id:Object.fromEntries(weapons.map((w,i)=>[w,['automatic','sniper','shotgun','vehicle','unknown'][i]]))};
const epoch=Date.parse('2026-09-01T00:00:00Z'),at=(s:number)=>new Date(epoch+s*1000).toISOString();
const source:any={match:{id:'match',started_at:at(0),map:'TestMap'},player:'player',end:at(1839),kills:[],batches:[],observations:[],progress:[]};
for(let t=0;t<=1830;t+=30){const kills:any[]=[];for(let k=0;k<4;k++){
 const e={ts:at(t+k*.1),event_id:`e${t}-${k}`,instance_id:'boot',match_row:'match',event_time:t+k*.1,map:'TestMap',killer_steam_id:k===3?'enemy':'player',victim_steam_id:k===3?'player':`victim${k}`,cause:weapons[(t/30+k)%weapons.length],distance_m:k===2?2501:120+t%80,distance_invalid:t%90===0&&k===1,headshot:k%2===0,suicide:t%150===0&&k===3,team_kill:t%210===0&&k===2,tags:['Headshot']};source.kills.push(e);kills.push({eventId:e.event_id,contextTags:k%2===0?['Event.Headshot','Event.Penetration']:[],});}
 source.batches.push({received_at:at(t+.4),instance_id:'boot',payload:{events:kills}});
 source.observations.push({received_at:at(t),endpoint:'/v1/status',payload:{factionScores:[{name:'Red',score:100},{name:'Blue',score:70}],metrics:{frameMs:12.5},lighting:'Day',experiences:['Infantry']}});
 const players=[{steamId:'player',kills:t/10,cash:500+t,pingMs:25,faction:t<900?'Red':'Blue'}];
 if(t%60===0)source.progress.push({observed_at:at(t),roster_size:30,players});else source.observations.push({received_at:at(t),endpoint:'/v1/players',payload:{roster_size:31,players}});
}
function changed(fn:(s:any)=>void){const s=structuredClone(source);fn(s);return s;}
const cases=[['continuous',source],['missing-tags',changed(s=>{s.batches=[];for(const e of s.kills){delete e.tags;delete e.headshot;}})],['stale-feed',changed(s=>{s.batches=s.batches.filter((b:any)=>Date.parse(b.received_at)<epoch+300000);s.kills=[];})],['dedup-invalid',changed(s=>{s.kills.push({...s.kills[0]});s.kills.push({...s.kills[1],event_id:'invalid',event_time:-1});s.kills.push({...s.kills[2],event_id:'other-match',match_row:'other'});})],['mixed-boot',changed(s=>{s.kills[5].instance_id='another';})],['mixed-map',changed(s=>{s.kills[5].map='other';})],['short-round',changed(s=>{s.match.started_at=at(100);})],['missing-roster',changed(s=>{s.observations=s.observations.filter((o:any)=>o.endpoint==='/v1/status');s.progress=[];})],['silence',changed(s=>{s.observations=[];s.progress=[];s.kills=[];s.batches=[];})]];
const output=cases.map(([name,s])=>{const rows=buildExpanded(s,contract);return {name,source:s,rows,input:Array.from(normalizeRows(rows,contract))};});
writeFileSync(new URL('./model-features.json',import.meta.url),JSON.stringify({contract,cases:output})+'\n');
console.log(`${output.length} expanded feature cases`);
