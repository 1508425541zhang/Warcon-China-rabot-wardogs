"""Independent database observer. No Feed job claims, mutations or game commands."""
import argparse
from collections import defaultdict
import datetime as dt
import json
import os
from pathlib import Path
import time
import numpy as np
import psycopg
from short_risk import ShortRiskModel, utc, scope
from unsupervised_model import UnsupervisedShortRisk

QUERY = """
WITH active AS (
 SELECT s.id FROM servers s JOIN integrity_rules r ON r.org_id=s.org_id
 JOIN organizations o ON o.id=s.org_id
 WHERE r.assessment_mode IN ('short_only','model_only') AND o.suspended_at IS NULL
)
SELECT json_build_object('now',now(),
 'live',(SELECT coalesce(json_agg(l),'[]') FROM server_live l JOIN active a ON a.id=l.server_id),
 'matches',(SELECT coalesce(json_agg(m),'[]') FROM matches m JOIN active a ON a.id=m.server_id WHERE ended_at IS NULL),
 'events',(SELECT coalesce(json_agg(k),'[]') FROM kills k JOIN active a ON a.id=k.server_id WHERE ts > now()-interval '10 minutes'));
"""

def atomic(path, value):
    tmp=path.with_suffix('.tmp')
    tmp.write_text(json.dumps(value,ensure_ascii=False,allow_nan=False),encoding='utf-8')
    os.replace(tmp,path)

def threshold(samples):
    if len(samples)<1000 or len({s['player'] for s in samples})<30:
        return None
    values=[s['score'] for s in samples]
    if max(values)-min(values)<1e-6:return None
    return float(np.quantile(values,.99,method='higher'))

def track_windows(status, history):
    now=utc(status['evaluated_at']); seen=set()
    for player in status['players']:
        scope_key=tuple(player.get('scope') or [])
        key=(player['server_id'],player['player_id'],scope_key)
        score=player.get('anomaly_score')
        if len(scope_key)!=4 or not isinstance(score,(int,float)) or not np.isfinite(score):
            continue
        seen.add(key)
        rows=history.get(key,[])
        if rows and (now-utc(rows[-1]['at'])>20 or now<=utc(rows[-1]['at'])):
            rows=[]
        rows=(rows+[{'at':status['evaluated_at'],'score':float(score)}])[-5:]
        history[key]=rows
        player['recent_windows']=rows
    for key in list(history):
        if key not in seen:del history[key]
    return status

def evaluate(model, data, reference):
    now=utc(data['now']);groups=defaultdict(list);results=[]
    for e in data['events']:groups[scope(e)].append(e)
    current={}
    for m in data['matches']:
        if m['server_id'] not in current or m['id']>current[m['server_id']]['id']:
            current[m['server_id']]=m
    samples=[s for s in reference if now-7*86400<s['time']<=now]
    additions=[]
    for live in data['live']:
        sid=live['server_id'];match=current.get(sid)
        candidates=[(g,es) for g,es in groups.items() if g[0]==sid and match and g[2]==str(match['id'])]
        selected=max(candidates,key=lambda pair:max(utc(e['ts']) for e in pair[1]),default=None)
        healthy=bool(live['ok']) and all(live.get(k) and 0<=now-utc(live[k])<=45
            for k in ('feed_at','status_at','players_at'))
        baseline=[s for s in samples if s['server']==sid]
        fixed=hasattr(model,'bundle')
        p99=model.threshold if fixed else threshold(baseline)
        # Include boundary ties only if they form at most 1% of the reference.
        meaningful=fixed or (p99 is not None and sum(s['score']>=p99 for s in baseline)/len(baseline)<=.011)
        for player in live.get('players') or []:
            pid=player.get('steamId')
            if not pid:continue
            result={'player_id':pid,'server_id':sid,'name':player.get('name'),
                'ShortRisk':None,'threshold_score':p99,'threshold_percentile':99.6 if fixed else 99,
                'candidate':False,'kick_executed':False,
                'execution_status':'handled_by_panel','status':'waiting_for_current_round_events'}
            if not healthy:result['status']='feed_or_state_unhealthy'
            elif selected:
                group,events=selected
                latest=max(events,key=lambda e:utc(e['ts']))
                elapsed=now-utc(latest['ts'])
                if elapsed>120 or elapsed<0:result['status']='game_clock_unavailable'
                else:
                    clock=float(latest['event_time'])+elapsed
                    pred=model.predict({'events':events,'player_id':pid,
                        'decision_received_utc':now,'decision_game_seconds':clock})
                    result.update(pred)
                    result['scope']=list(group)
                    score=pred['ShortRisk']
                    if score is not None:
                        result['candidate']=pred['tail_candidate'] if fixed else bool(meaningful and score>=p99)
                        result['reference_status']='ready' if meaningful else 'insufficient_or_tied_reference'
                        key=(sid,pid,group[1],group[2])
                        prior=max((s['time'] for s in baseline if (s['server'],s['player'],s['instance'],s['match'])==key),default=0)
                        if not fixed and now-prior>=120:
                            additions.append({'server':sid,'player':pid,'instance':group[1],
                                'match':group[2],'time':now,'score':score})
            results.append(result)
    return {'evaluated_at':data['now'],'mode':'independent_readonly_observer',
        'requested_policy':'warning_at_p996_kick_at_p999','effective_policy':'panel_warning_p996_kick_p999',
        'model_sha256':model.metadata['model_sha256'],'model_readiness':model.metadata['readiness'],
        'threshold_meaning':'upper 0.4% of fixed held-out reference scores; not cheating probability',
        'threshold_percentile':99.6,'algorithm':'IsolationForest' if hasattr(model,'bundle') else 'legacy',
        'reference_samples':len(model.bundle['reference_scores']) if hasattr(model,'bundle') else len(samples),
        'players':results},samples+additions

def main():
    p=argparse.ArgumentParser();p.add_argument('--model',type=Path,required=True)
    p.add_argument('--state-dir',type=Path,required=True);a=p.parse_args()
    model=None;a.state_dir.mkdir(parents=True,exist_ok=True)
    refpath=a.state_dir/'reference.json';reference=[];history={}
    while True:
        started=time.monotonic()
        try:
            with psycopg.connect(dbname=os.environ['PGDATABASE'],user='warcon_short_risk',
                    host='/var/run/postgresql',connect_timeout=5,
                    options='-c default_transaction_read_only=on -c statement_timeout=5000') as con:
                with con.transaction():
                    con.execute('SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY')
                    data=con.execute(QUERY).fetchone()[0]
            if not data['live']:
                model=None;reference=[];history.clear()
                atomic(a.state_dir/'status.json',{'mode':'inactive','status':'engine_not_selected',
                    'evaluated_at':data['now'],'players':[],'kick_executed':False})
                time.sleep(max(.1,10-(time.monotonic()-started)))
                continue
            if model is None:model=UnsupervisedShortRisk(a.model,quantile=.996)
            status,reference=evaluate(model,data,reference)
            track_windows(status,history)
            atomic(a.state_dir/'status.json',status)
            atomic(refpath,{'model_sha256':model.metadata['model_sha256'],'samples':reference})
            print(json.dumps({'players':len(status['players']),'reference_samples':status['reference_samples'],
                'candidates':sum(r['candidate'] for r in status['players']),
                'execution_status':'handled_by_panel'}),flush=True)
        except Exception as exc:
            history.clear()
            atomic(a.state_dir/'status.json',{'mode':'independent_readonly_observer',
                'status':'observer_error','error_type':type(exc).__name__,'players':[],
                'kick_executed':False,'evaluated_at':dt.datetime.now(dt.timezone.utc).isoformat()})
            print('Observer failure: '+type(exc).__name__,flush=True)
        time.sleep(max(.1,10-(time.monotonic()-started)))

if __name__=='__main__':main()
