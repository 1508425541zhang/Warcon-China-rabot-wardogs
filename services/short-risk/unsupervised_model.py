"""Trusted local model inference; percentile is not a cheating probability."""
import argparse
import json
from pathlib import Path
import time
import joblib
import numpy as np
from short_risk import FEATURES,digest,normalize,scope,feature_vector,utc

class UnsupervisedShortRisk:
    def __init__(self,folder,quantile=.996):
        report=json.loads((folder/'report.json').read_text(encoding='utf-8'))
        if digest(folder/'model.joblib')!=report['model_sha256']:raise ValueError('Checksum mismatch')
        # joblib files must only come from this project's trusted training output.
        self.bundle=joblib.load(folder/'model.joblib')
        if self.bundle['features']!=FEATURES:raise ValueError('Feature mismatch')
        self.quantile=quantile
        self.threshold=float(np.quantile(self.bundle['reference_scores'],quantile,method='higher'))
        self.kick_threshold=float(np.quantile(self.bundle['reference_scores'],.999,method='higher'))
        self.metadata={'model_sha256':report['model_sha256'],'readiness':report['readiness']}

    def predict(self,request):
        start=time.perf_counter();events=[];keys={}
        for raw in request['events']:
            e=normalize(raw);key=(e['server_id'],e['instance_id'],e['event_id'])
            if key in keys:
                if e!=keys[key]:raise ValueError('Conflicting event')
                continue
            keys[key]=e;events.append(e)
        if len({scope(e) for e in events})>1:raise ValueError('Round isolation required')
        pid=request['player_id'];clock=request['decision_game_seconds'];now=utc(request['decision_received_utc'])
        if not any(e['_received']<=now and clock-120<e['_clock']<=clock and
            pid in (e.get('killer_steam_id'),e.get('victim_steam_id')) for e in events):
            return {'ShortRisk':None,'status':'insufficient_data','next_update_seconds':10}
        vector=feature_vector(events,pid,clock,now,self.bundle['distance_baseline'])
        transformed=self.bundle['imputer'].transform(vector.reshape(1,-1))
        score=float(-self.bundle['model'].score_samples(transformed)[0])
        ref=self.bundle['reference_scores']
        percentile=float(np.searchsorted(ref,score,side='left')/len(ref))
        return {'ShortRisk':percentile,'anomaly_score':score,'percentile':percentile*100,
            'threshold_percentile':self.quantile*100,'threshold_score':self.threshold,
            'tail_candidate':bool(score>self.threshold),
            'kick_threshold_score':self.kick_threshold,'kick_candidate':bool(score>self.kick_threshold),
            'status':'unsupervised_shadow','calibrated':False,'next_update_seconds':10,
            'meaning':'reference distribution percentile, not cheating probability',
            'inference_ms':(time.perf_counter()-start)*1000,'feature_count':len(FEATURES)}

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--model',type=Path,required=True)
    p.add_argument('--input',type=Path,required=True);a=p.parse_args()
    print(json.dumps(UnsupervisedShortRisk(a.model).predict(json.loads(a.input.read_text(encoding='utf-8')))))
