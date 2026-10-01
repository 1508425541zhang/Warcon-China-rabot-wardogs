"""Offline conversion of trusted training outputs. Rust deployment never loads pickle/Python.

The original report SHA must match before loading the locally trained joblib artifact.
Do not use this tool on downloaded, untrusted pickle/joblib files.
"""
import argparse
import hashlib
import json
from pathlib import Path
import sys
import numpy as np
import joblib
import xgboost as xgb

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'services' / 'short-risk'))
from short_risk import FEATURES, feature_vector, normalize

def digest(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, allow_nan=False, separators=(',', ':'))+'\n', encoding='utf-8', newline='\n')
    return digest(path)

def export_isolation(folder, target):
    report=json.loads((folder/'report.json').read_text(encoding='utf-8'))
    if digest(folder/'model.joblib')!=report['model_sha256']: raise ValueError('Source model checksum mismatch')
    bundle=joblib.load(folder/'model.joblib')
    if bundle['features']!=FEATURES: raise ValueError('Feature contract mismatch')
    model=bundle['model']
    from sklearn.ensemble._iforest import _average_path_length
    trees=[]
    for i,t in enumerate(model.estimators_):
        tree=t.tree_
        trees.append({'left':tree.children_left.tolist(),'right':tree.children_right.tolist(),
            'feature':tree.feature.tolist(),'threshold':tree.threshold.tolist(),
            'feature_map':model.estimators_features_[i].tolist(),
            'leaf_depth':(model._decision_path_lengths[i]+model._average_path_length_per_tree[i]-1).tolist()})
    artifact={'format':'warcon-short-native-v1','algorithm':'IsolationForest','features':FEATURES,
        'source_model_sha256':report['model_sha256'],'readiness':report['readiness'],
        'imputer':bundle['imputer'].statistics_.tolist(),'distance_baseline':bundle['distance_baseline'],
        'reference_scores':bundle['reference_scores'].tolist(),'trees':trees,
        'denominator':len(trees)*float(_average_path_length([model._max_samples])[0])}
    return artifact, lambda x: float(-model.score_samples(bundle['imputer'].transform(x.reshape(1,-1)))[0])

def export_xgboost(folder,target):
    report=json.loads((folder/'metadata.json').read_text(encoding='utf-8'))
    if digest(folder/'model.ubj')!=report['model_sha256'] or report['features']!=FEATURES: raise ValueError('Source model checksum/feature mismatch')
    booster=xgb.Booster();booster.load_model(folder/'model.ubj')
    data=json.loads(booster.save_raw(raw_format='json'))['learner']
    if data['objective']['name']!='binary:logistic':raise ValueError('Unsupported XGBoost objective')
    base=float(data['learner_model_param']['base_score'].strip('[]'))
    if not 0<base<1:raise ValueError('Invalid XGBoost base score')
    trees=[]
    for t in data['gradient_booster']['model']['trees']:
        if any(t['split_type']):raise ValueError('Categorical trees are unsupported')
        trees.append({'left':t['left_children'],'right':t['right_children'],'feature':t['split_indices'],
            'threshold':t['split_conditions'],'missing_left':[bool(v) for v in t['default_left']]})
    artifact={'format':'warcon-short-native-v1','algorithm':'XGBoost','features':FEATURES,
        'source_model_sha256':report['model_sha256'],'readiness':report['readiness'],
        'distance_baseline':report['distance_baseline'],'trees':trees,'base_score':base}
    return artifact,lambda x:float(booster.predict(xgb.DMatrix(x.reshape(1,-1)))[0])

def main():
    p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);p.add_argument('--algorithm',choices=['isolation','xgboost'],required=True)
    p.add_argument('--output',type=Path,required=True);p.add_argument('--fixtures',type=Path,required=True);a=p.parse_args()
    artifact,score=(export_isolation if a.algorithm=='isolation' else export_xgboost)(a.source,a.output)
    checksum=write(a.output,artifact);write(a.output.with_suffix('.manifest.json'),{'format':artifact['format'],'model_sha256':checksum,'source_model_sha256':artifact['source_model_sha256'],'algorithm':artifact['algorithm']})
    cases=[];vectors=[]
    epoch=1788220800.
    for count in [0,1,5,12,30,75]:
        for head in [0.,.6,1.]:
            events=[]
            for i in range(count):
                e={'ts':epoch+100+i*.25,'event_time':100+i*.25,'event_id':f'e{i}','server_id':'fixture-server','instance_id':'fixture-boot','match_row':27,'map':'fixture-map',
                    'killer_steam_id':'player' if i%5 else 'other','victim_steam_id':f'victim{i%7}' if i%5 else 'player','cause':'Id.Item.AK74M' if i%4 else 'unknown',
                    'headshot':(i%10)<int(head*10),'tags':['Penetration'] if i%7==0 else [],'suicide':False,'team_kill':False,'distance_m':10+i*9,'distance_invalid':i%9==0}
                events.append(e)
            request={'events':events,'player_id':'player','decision_game_seconds':130.,'decision_received_utc':epoch+130}
            vector=feature_vector([normalize(e) for e in events],'player',130.,epoch+130,artifact['distance_baseline'])
            cases.append({'request':request,'features':[None if np.isnan(v) else float(v) for v in vector]})
    rng=np.random.default_rng(20261001)
    for i in range(128):
        vector=rng.normal(1,5,len(FEATURES)).astype(np.float32);vector[rng.random(len(FEATURES))<.15]=np.nan
        vectors.append({'features':[None if np.isnan(v) else float(v) for v in vector],'score':score(vector)})
    observers=[]
    if a.algorithm=='isolation':
        from unsupervised_model import UnsupervisedShortRisk
        from cloud_observer import evaluate,track_windows
        model=UnsupervisedShortRisk(a.source);history={}
        for tick in [130,140,150,160,170,220,230,240,250]:
            data={'now':epoch+tick,'matches':[{'server_id':'fixture-server','id':27 if tick<230 else 28}],
                'events':cases[16]['request']['events'],
                'live':[{'server_id':'fixture-server','ok':True,'feed_at':epoch+tick-1,'status_at':epoch+tick-1,'players_at':epoch+tick-1,'players':[{'steamId':'player','name':'fixture'}]}]}
            if tick==240:data['live'][0]['feed_at']=epoch
            result,_=evaluate(model,data,[]);track_windows(result,history)
            for player in result['players']:player.pop('inference_ms',None)
            observers.append({'data':data,'result':result})
    sources=['services/short-risk/short_risk.py','services/short-risk/unsupervised_model.py','services/short-risk/cloud_observer.py']
    write(a.fixtures,{'algorithm':artifact['algorithm'],'source_model_sha256':artifact['source_model_sha256'],'original_sources':{s:digest(ROOT/s) for s in sources},'feature_cases':cases,'vectors':vectors,'observers':observers})
    print(json.dumps({'algorithm':artifact['algorithm'],'trees':len(artifact['trees']),'features':len(FEATURES),'model_bytes':a.output.stat().st_size,'parity_vectors':len(vectors)}))
if __name__=='__main__':main()
