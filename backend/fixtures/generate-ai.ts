import { writeFileSync } from 'node:fs';
import { reviewOutput, numericChecks, SYSTEM, PROMPT_VERSION, settingsInput, apiBase } from '../../src/lib/server/integrity/ai-protocol';
// Load the original pure triage body without importing the database runtime.
import { readFileSync } from 'node:fs';
const source=readFileSync(new URL('../../src/lib/server/integrity/ai-triage.ts',import.meta.url),'utf8');
const body=source.slice(source.indexOf('export function triageDecision'),source.indexOf('/** Lock in case'));
const js=await Bun.Transpiler.prototype.transform.call(new Bun.Transpiler({loader:'ts'}),body.replace('export function','function')+'\ntriageDecision;');
const triage=Function('reviewOutput',js+'\nreturn triageDecision;')(reviewOutput);
const basic={verdict:'建议通过',suspicionPercent:20,evidenceQuality:'中',alternatives:['有正常解释'],summary:'资料不足以支持异常',reasons:[{text:'核对数值',evidence:'case.snapshot'}],contradictions:[],missingEvidence:[]};
const reviews=[];
for(const percent of [null,0,20,25,30,60,65,95,100,101,21])for(const quality of ['低','中','高'])for(const verdict of ['建议通过','建议复核','证据不足']){
 const input={...basic,suspicionPercent:percent,evidenceQuality:quality,verdict}; const p=reviewOutput.safeParse(input);
 reviews.push({input,valid:p.success,expected:p.success?[triage(input,true),triage(input,false)]:null});
}
for(const delta of [{reasons:[]},{reasons:[{text:'数值',evidence:' '}]},{contradictions:['矛盾']},{missingEvidence:['关键缺失']},{summary:''},{suspicionPercent:undefined},{verdict:'封禁'}]){
 const input={...basic,...delta}; const p=reviewOutput.safeParse(input);reviews.push({input,valid:p.success,expected:p.success?[triage(input,true),triage(input,false)]:null});
}
const snapshots=[null,{}, {infantryKills:6,kpm180:2},{infantryKills:6,kpm180:2.01},{infantryKills:6,kpm180:2.02},{infantryKills:'6',kpm180:2},{infantryKills:7,kpm180:2.33333333}].map(input=>({input,expected:numericChecks(input)}));
const urls=['https://api.example.com/v1/','https://api.example.com','http://api.example.com','https://user:pw@example.com','https://api.example.com/v1/models','https://api.example.com/v1/chat/completions/','https://api.example.com/v1?key=abc','https://api.example.com/v1#hash'].map(input=>{try{return {input,expected:apiBase(input)}}catch{return {input,expected:null}}});
writeFileSync(new URL('ai.json',import.meta.url),JSON.stringify({reviews,snapshots,urls,promptVersion:PROMPT_VERSION,system:SYSTEM},null,2)+'\n');
