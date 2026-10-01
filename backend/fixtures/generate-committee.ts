// Capture the existing implementation, before any routing is switched to Rust.
import { writeFileSync } from 'node:fs';
import { assessCommittee, voteCommittee, hasDirectKickSupport, hasStatisticalAnomaly, shouldSaveCommitteeAssessment } from '../../src/lib/server/integrity/committee';
import { STATISTICAL_MODEL_CONFIG as config } from '../../src/lib/server/integrity/statistical-config';
const quality = {feedHealthy:true,backlogSafe:true,identityReliable:true,roundReliable:true,baselineFresh:true,versionsMatch:true,baselinePopulationAdequate:true};
const base = () => ({statistical:{status:'READY',modelVersion:config.modelVersion,featureVersion:config.featureVersion,metrics:[]},precision:[],independentEpisodes:0,currentKpm:1,eventIds:['fixture-1']});
const assessments:any[]=[];
const add=(input:any,health=quality)=>assessments.push({input,quality:health,expected:assessCommittee(input,health)});
add(base());
for(const p of [0.899,0.9,0.949,0.95,0.999])for(const n of [49,50,200]){const input=base();input.statistical.metrics=[{source:'local',code:'kpm180',sampleCount:n,extremenessPercentile:p}] as any;add(input)}
for(const category of ['automatic','sniper','shotgun','unknown'])for(const [kills,headshots] of [[4,4],[5,3],[10,7],[10,9]]){const input:any=base();input.precision=[{category,kills,headshots,rate:headshots/kills,peerPlayers:20,peerKills:200,peerHeadshots:30,serverRate:0.15,difference:headshots/kills-0.15,ratio:headshots/kills/0.15,percentile:0.8}];add(input)}
for(const kpm of [2,2.01,4,4.01,9]){const input:any=base();input.currentKpm=kpm;input.independentEpisodes=2;add(input);add(input,{...quality,feedHealthy:false})}
for(const kpm of [1,8,16]){const input:any=base();input.currentKpm=kpm;input.career={sampleCount:100,uniqueDays:10,kpmMedian:1,kpmMad:0.5,recentKpm:[8,8,8]};input.changeSeries=[...Array.from({length:20},(_,i)=>i%3),8,9,8];add(input)}
const votes:any[]=[];
const decisions=['UNKNOWN','NORMAL','SUSPICIOUS','CHEAT_LIKELY'];
const verdict=(id:string,decision:string)=>({modelId:id,modelVersion:'1',evidenceFamily:'TEMPO',decision,confidence:1,evidenceQuality:1,reasons:[],evidenceRefs:['e']});
for(const a of decisions)for(const b of decisions)for(const c of decisions){const ballots=[verdict('tempo',a),verdict('career_deviation',b),verdict('precision',c),verdict('persistence','NORMAL'),verdict('change_point','UNKNOWN')];votes.push({verdicts:ballots,veto:[],expected:voteCommittee(ballots),direct:hasDirectKickSupport(ballots,4.01)})}
for(const veto of [[],['FEED_STALE']]) {const ballots=[verdict('tempo','CHEAT_LIKELY'),verdict('tempo','CHEAT_LIKELY'),verdict('precision','SUSPICIOUS')];votes.push({verdicts:ballots,veto,expected:voteCommittee(ballots,veto),direct:hasDirectKickSupport(ballots,4.01)})}
const saved=assessments.map(({input,expected})=>{const assessment={...input.statistical,committee:expected};return {assessment,anomaly:hasStatisticalAnomaly(assessment as any),save:shouldSaveCommitteeAssessment(assessment as any)}});
writeFileSync(new URL('./committee.json',import.meta.url),JSON.stringify({assessments,votes,saved},null,2)+'\n');
console.log(JSON.stringify({assessments:assessments.length,votes:votes.length,saved:saved.length}));
