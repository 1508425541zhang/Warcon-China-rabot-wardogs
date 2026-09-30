"""Small causal 60/120-second XGBoost risk model. CPU-only, no game actions."""
from __future__ import annotations
import argparse
from bisect import bisect_right
from collections import Counter, defaultdict
import datetime as dt
import gzip
import hashlib
import json
import math
from pathlib import Path
import time
import numpy as np
import xgboost as xgb
from sklearn.metrics import roc_auc_score, average_precision_score, brier_score_loss, log_loss

VERSION='short-risk-24-v1'
WINDOW_FEATURES=['small_arm_kills','kills','deaths','headshot_rate','penetration_rate',
 'unique_victims','max_kills_15s','median_interval_s','distance_ratio_p90',
 'headshot_samples','distance_samples']
FEATURES=[f'{f}_{w}s' for w in (60,120) for f in WINDOW_FEATURES]+['kill_rate_change','dominant_weapon_fraction_120s']
WEAPONS={f'Id.Item.{s}' for s in ['AK74M','Mosin','MP9','WEPN_029','M4','M500','MP43',
 'SKS','SVDM','KH2002','TAR21','A91','SV98','MK22','Glock17','CombatBow']}
LABELS={'CONFIRMED_ABUSE':1,'FALSE_POSITIVE':0}

def utc(s):
    if isinstance(s,(int,float)):return float(s)
    x=dt.datetime.fromisoformat(s.replace('Z','+00:00'))
    if x.tzinfo is None:raise ValueError('UTC offset required')
    return x.timestamp()

def finite(x):
    return isinstance(x,(int,float)) and not isinstance(x,bool) and math.isfinite(x)

def digest(path):
    h=hashlib.sha256()
    with path.open('rb') as f:
        for b in iter(lambda:f.read(1024*1024),b''):h.update(b)
    return h.hexdigest()

def valid_attack(e,player):
    tags=e.get('tags') or []
    return (e.get('killer_steam_id')==player and e.get('victim_steam_id')!=player
            and not e.get('suicide') and not e.get('team_kill')
            and not any(t in tags for t in ('Suicide','Falling','RoadKill')))

def weapon_attack(e,player):return valid_attack(e,player) and e.get('cause') in WEAPONS

def normalize(e):
    result=dict(e)
    result['_received']=utc(e['ts'])
    if not finite(e.get('event_time')) or e['event_time']<0:raise ValueError('Invalid game clock')
    result['_clock']=float(e['event_time'])
    return result

def scope(e):
    # Ingest match_row can be uncertain around travel; keep it explicit and never cross it.
    return (e['server_id'],e['instance_id'],str(e.get('match_row')),e.get('map') or '')

def baseline(events,cutoff):
    values=defaultdict(list)
    for e in events:
        d=e.get('distance_m');p=e.get('killer_steam_id')
        if e['_received']<=cutoff and p and weapon_attack(e,p) and finite(d) and 0<=d<=2000 and not e.get('distance_invalid'):
            values[e['cause']].append(d)
    return {w:{'p95_m':max(float(np.percentile(v,95)),1.0),'samples':len(v)}
            for w,v in sorted(values.items()) if len(v)>=10}

def feature_vector(events,player,now_clock,now_received,reference):
    if not finite(now_clock) or not finite(now_received):raise ValueError('Finite decision clocks required')
    # Two clocks: a late event cannot enter a decision made before it was received.
    available=[e for e in events if e['_received']<=now_received and e['_clock']<=now_clock
               and e['_clock']>now_clock-120]
    values=[];rates=[];latest_weapons=[]
    for window in (60,120):
        recent=[e for e in available if e['_clock']>now_clock-window]
        attacks=[e for e in recent if valid_attack(e,player)]
        arms=sorted((e for e in recent if weapon_attack(e,player)),key=lambda e:(e['_clock'],e['event_id']))
        deaths=sum(e.get('victim_steam_id')==player for e in recent)
        # Unknown flags stay missing; use the normalized source flag only when tags are present.
        head=[e.get('headshot') for e in arms if isinstance(e.get('tags'),list) and isinstance(e.get('headshot'),bool)]
        pen=[bool(e.get('penetration')) if isinstance(e.get('penetration'),bool)
             else any('Penetration' in str(t) for t in e['tags']) for e in arms if isinstance(e.get('tags'),list)]
        intervals=np.diff([e['_clock'] for e in arms])
        burst=0;left=0
        for right,e in enumerate(arms):
            while e['_clock']-arms[left]['_clock']>15:left+=1
            burst=max(burst,right-left+1)
        distances=[]
        for e in arms:
            d=e.get('distance_m');b=reference.get(e.get('cause'))
            if b and finite(d) and 0<=d<=2000 and not e.get('distance_invalid'):
                distances.append(d/b['p95_m'])
        values.extend([len(arms),len(attacks),deaths,np.mean(head) if head else np.nan,
            np.mean(pen) if pen else np.nan,len({e.get('victim_steam_id') for e in arms}),burst,
            np.median(intervals) if len(intervals) else np.nan,
            np.percentile(distances,90) if distances else np.nan,len(head),len(distances)])
        rates.append(len(arms))
        if window==120:latest_weapons=[e['cause'] for e in arms]
    values.extend([rates[0]-(rates[1]-rates[0]),
        max(Counter(latest_weapons).values())/len(latest_weapons) if latest_weapons else np.nan])
    return np.asarray(values,dtype=np.float32)

def read_snapshot(path):
    tables=defaultdict(list)
    with gzip.open(path,'rt',encoding='utf-8') as f:
        for line in f:
            r=json.loads(line);tables[r['table']].append(r['row'])
    return tables

def prepare(path):
    tables=read_snapshot(path);events=[];keys={};rejected=Counter()
    for raw in tables['kills']:
        try:e=normalize(raw)
        except (ValueError,KeyError):rejected['invalid_clock']+=1;continue
        if e.get('match_row') is None:rejected['missing_round']+=1;continue
        key=(e['server_id'],e['instance_id'],e['event_id'])
        if key in keys:
            if raw!=keys[key]['raw']:raise ValueError('Conflicting event ID')
            rejected['duplicate_events']+=1;continue
        keys[key]={'raw':raw,'event':e};events.append(e)
    events.sort(key=lambda e:(scope(e),e['_clock'],e['event_id']))
    by_scope=defaultdict(list)
    for e in events:by_scope[scope(e)].append(e)
    case_keys=defaultdict(list)
    for ce in tables['case_events']:case_keys[ce['case_id']].append((ce['instance_id'],ce['event_id']))
    samples=[]
    for label in tables['labels']:
        if label.get('label') not in LABELS:rejected['insufficient_evidence_labels']+=1;continue
        if not label.get('reviewer_id'):continue
        linked=[keys[(label['server_id'],inst,eid)]['event'] for inst,eid in case_keys[label['case_id']]
                if (label['server_id'],inst,eid) in keys]
        if not linked:rejected['case_has_no_matched_events']+=1;continue
        # One endpoint per case: 10-second ticks do not create independent labels.
        last=max(linked,key=lambda e:e['_received']);decision=math.ceil(last['_received']/10)*10
        group=by_scope[scope(last)]
        observed=[e for e in group if e['_received']<=decision]
        latest=max(observed,key=lambda e:e['_clock'])
        clock=latest['_clock']+max(0,decision-latest['_received'])
        if not any(weapon_attack(e,label['steam_id']) and clock-120<e['_clock']<=clock for e in observed):
            rejected['case_no_short_window_small_arm_kills']+=1;continue
        samples.append({'case_id':label['case_id'],'player':label['steam_id'],'scope':scope(last),
            'decision_received':decision,'decision_clock':clock,'label':LABELS[label['label']],
            'reviewed_at':utc(label['created_at'])})
    samples.sort(key=lambda s:(s['decision_received'],s['case_id']))
    conflict=set()
    for i,a in enumerate(samples):
        for j,b in enumerate(samples[:i]):
            if a['player']==b['player'] and a['scope']==b['scope'] and abs(a['decision_clock']-b['decision_clock'])<120 and a['label']!=b['label']:
                conflict.update([i,j])
    rejected['conflicting_label_windows']=len(conflict)
    samples=[s for i,s in enumerate(samples) if i not in conflict]
    return events,by_scope,samples,dict(rejected)

def evaluate(model,x,y):
    if not len(y):return {'samples':0}
    p=model.predict_proba(x)[:,1]
    both=len(set(y))==2
    return {'samples':len(y),'positive':int(sum(y)),
        'brier':float(brier_score_loss(y,p)),'log_loss':float(log_loss(y,p,labels=[0,1])),
        'roc_auc':float(roc_auc_score(y,p)) if both else None,
        'average_precision':float(average_precision_score(y,p)) if both else None}

def train(source,output,trees=50,depth=4):
    if not 30<=trees<=80 or not 3<=depth<=5:raise ValueError('Use 30–80 trees, depth 3–5')
    if output.exists():raise FileExistsError('Refusing to overwrite model directory')
    events,groups,samples,rejected=prepare(source)
    if len(samples)<4 or len({s['label'] for s in samples})<2:raise ValueError('Not enough matched positive/negative cases')
    # Later cases are reserved. Purge overlap and cases for training players from holdout.
    cut=max(2,int(len(samples)*.7));train_rows=samples[:cut];holdout=samples[cut:]
    train_end=max(s['decision_received'] for s in train_rows)
    train_players={s['player'] for s in train_rows}
    independent=[s for s in holdout if s['decision_received']-120>train_end and s['player'] not in train_players
                 and s['reviewed_at']>train_end]
    # If chronology leaves one class in training, fit all labelled cases as exploration only.
    fallback=len({s['label'] for s in train_rows})<2
    if fallback:train_rows=samples;independent=[];train_end=max(s['decision_received'] for s in samples)
    train_players={s['player'] for s in train_rows}
    reference=baseline(events,train_end)
    def matrix(rows):
        return np.stack([feature_vector(groups[s['scope']],s['player'],s['decision_clock'],
                                      s['decision_received'],reference) for s in rows]) if rows else np.empty((0,len(FEATURES)),np.float32)
    x=matrix(train_rows);y=np.asarray([s['label'] for s in train_rows])
    # Equal total weight per reviewed player; multiple cases do not multiply identity weight.
    counts=Counter(s['player'] for s in train_rows)
    weights=np.asarray([1/counts[s['player']] for s in train_rows]);weights*=len(weights)/sum(weights)
    # Logistic Hessian is at most 0.25 per unit weight. The small cohort needs a
    # lower leaf-weight floor to allow any split; choose using training size only.
    child_weight=.1 if len(train_rows)<20 else 1.0
    model=xgb.XGBClassifier(n_estimators=trees,max_depth=depth,learning_rate=.08,tree_method='hist',
        objective='binary:logistic',eval_metric='logloss',min_child_weight=child_weight,reg_lambda=3,
        subsample=1,colsample_bytree=1,n_jobs=2,random_state=20261001)
    start=time.perf_counter();model.fit(x,y,sample_weight=weights);elapsed=time.perf_counter()-start
    output.mkdir(parents=True)
    model.save_model(output/'model.ubj')
    data_x=matrix(samples)
    np.savez_compressed(output/'labelled_windows.npz',X=data_x,y=np.asarray([s['label'] for s in samples]),
        split=np.asarray(['train' if s in train_rows else 'holdout' if s in independent else 'excluded_holdout' for s in samples]))
    report={'version':VERSION,'output':'ShortRisk','range':[0,1],'features':FEATURES,
        'window_seconds':[60,120],'update_seconds':10,'trees':trees,'max_depth':depth,'objective':'binary:logistic',
        'min_child_weight':child_weight,
        'source_sha256':digest(source),'events':len(events),'matched_labelled_cases':len(samples),
        'train_cases':len(train_rows),'train_players':len(train_players),
        'case_label_counts':dict(Counter(str(s['label']) for s in samples)),
        'independent_holdout_cases':len(independent),'excluded_holdout_cases':len(holdout)-len(independent),
        'training_only_fallback':fallback,'training_seconds':elapsed,'model_bytes':(output/'model.ubj').stat().st_size,
        'model_sha256':digest(output/'model.ubj'),
        'trees_with_splits':sum('"split"' in tree for tree in model.get_booster().get_dump(dump_format='json')),
        'holdout_metrics':evaluate(model,matrix(independent),np.asarray([s['label'] for s in independent])),
        'train_fit_metrics':evaluate(model,x,y),'distance_baseline':reference,
        'distance_baseline_cutoff_received_utc':dt.datetime.fromtimestamp(train_end,dt.timezone.utc).isoformat(),
        'readiness':'experimental_advisory_only','calibrated':False,
        'rejected_source_or_labels':rejected,
        'holdout_method':'retrospective chronological case holdout, 120-second purge and unseen players; not a simulated deployment before review labels existed',
        'limitations':['Case-level human labels are weak labels for the final short window, not event-level cheating truth.',
          'The small, case-selected cohort is not representative of all players. Unreviewed players are not negative labels.',
          'No automatic punishment is authorized by this model. ShortRisk is uncalibrated.',
          'Match-row assignment is ingestion context. Windows never cross its boundaries; travel-delayed events may remain uncertain.',
          'Small-arm weapon classification does not assert infantry faction correctness.',
          'No-hit, damage, ammunition, ping, cash and playtime inputs are fabricated.']}
    (output/'metadata.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
    public=[{k:v for k,v in s.items() if k not in ('player','case_id')} for s in samples]
    (output/'sample_index.json').write_text(json.dumps(public,indent=2),encoding='utf-8')
    print(json.dumps({k:report[k] for k in ['events','matched_labelled_cases','train_cases','independent_holdout_cases','excluded_holdout_cases','model_bytes','training_seconds','holdout_metrics','readiness']}))
    return report

class ShortRiskModel:
    def __init__(self,folder):
        self.metadata=json.loads((folder/'metadata.json').read_text(encoding='utf-8'))
        if self.metadata['version']!=VERSION or self.metadata['features']!=FEATURES:raise ValueError('Feature contract mismatch')
        if self.metadata.get('model_sha256') and digest(folder/'model.ubj')!=self.metadata['model_sha256']:
            raise ValueError('Model checksum mismatch')
        self.model=xgb.XGBClassifier();self.model.load_model(folder/'model.ubj')

    def predict(self,request):
        events=[normalize(e) for e in request['events']]
        keys={};dedup=[]
        for e in events:
            k=(e['server_id'],e['instance_id'],e['event_id'])
            if k in keys:
                if e!=keys[k]:raise ValueError('Conflicting event ID')
            else:keys[k]=e;dedup.append(e)
        if len({scope(e) for e in dedup})>1:raise ValueError('One server / instance / round / map per request')
        now=utc(request['decision_received_utc']);clock=request['decision_game_seconds']
        feature=feature_vector(dedup,request['player_id'],clock,now,self.metadata['distance_baseline'])
        has_source=any(e['_received']<=now and clock-120<e['_clock']<=clock
            and request['player_id'] in (e.get('killer_steam_id'),e.get('victim_steam_id')) for e in dedup)
        if not has_source:return {'ShortRisk':None,'status':'insufficient_data','next_update_seconds':10}
        start=time.perf_counter();risk=float(self.model.predict_proba(feature.reshape(1,-1))[0,1])
        return {'ShortRisk':risk,'status':'experimental','calibrated':False,'next_update_seconds':10,
            'inference_ms':(time.perf_counter()-start)*1000,'feature_count':len(FEATURES)}

class RollingShortRisk:
    """Buffer source events; caller supplies authoritative round and clocks on each 10s tick.

    The caller should stop ticks while feed health is unknown. Missing feed intervals must
    not be converted into zero kills. Timestamp order, not ingestion order, drives features.
    """
    def __init__(self,model):
        self.model=model;self.events=defaultdict(dict);self.latest_received={};self.cache={}

    def ingest(self,event):
        e=normalize(event);group=scope(e);key=(e['instance_id'],e['event_id'])
        prior=self.events[group].get(key)
        if prior is not None and prior!=event:raise ValueError('Conflicting duplicate source event')
        self.events[group][key]=dict(event)
        newest=max(e['_received'],self.latest_received.get(group,e['_received']))
        self.latest_received[group]=newest
        for k,v in list(self.events[group].items()):
            if utc(v['ts'])<newest-600:self.events[group].pop(k)
        for g,t in list(self.latest_received.items()):
            if t<newest-600:
                self.events.pop(g,None);self.latest_received.pop(g,None)
                for cache_key in list(self.cache):
                    if cache_key[0]==g:self.cache.pop(cache_key,None)

    def tick(self,group,player_id,game_seconds,received_utc,feed_healthy=True):
        now=utc(received_utc);key=(tuple(group),player_id)
        tick=math.floor(now/10)*10
        if not feed_healthy:
            self.cache.pop(key,None)
            return {'ShortRisk':None,'status':'feed_unhealthy','next_update_seconds':10}
        previous=self.cache.get(key)
        if previous and previous[0]==tick:return previous[1]
        result=self.model.predict({'events':list(self.events.get(tuple(group),{}).values()),'player_id':player_id,
            'decision_game_seconds':game_seconds,'decision_received_utc':received_utc})
        self.cache[key]=(tick,result)
        return result

if __name__=='__main__':
    p=argparse.ArgumentParser();sub=p.add_subparsers(dest='command',required=True)
    t=sub.add_parser('train');t.add_argument('--source',type=Path,required=True);t.add_argument('--output',type=Path,required=True)
    t.add_argument('--trees',type=int,default=50);t.add_argument('--depth',type=int,default=4)
    i=sub.add_parser('predict');i.add_argument('--model',type=Path,required=True);i.add_argument('--input',type=Path,required=True)
    args=p.parse_args()
    if args.command=='train':train(args.source,args.output,args.trees,args.depth)
    else:print(json.dumps(ShortRiskModel(args.model).predict(json.loads(args.input.read_text(encoding='utf-8')))))
