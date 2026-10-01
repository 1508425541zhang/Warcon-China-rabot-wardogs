import unittest
from cloud_observer import threshold,evaluate,track_windows

class Model:
    metadata={'model_sha256':'test','readiness':'experimental_advisory_only'}
    def predict(self,request):return {'ShortRisk':.9,'status':'experimental'}

class ObserverTests(unittest.TestCase):
    def test_windows_reset_on_gap_round_change_and_unavailable_score(self):
        history={}
        def sample(at,scope=('s','i','1','map'),score=.8):
            return track_windows({'evaluated_at':at,'players':[{'server_id':'s','player_id':'p','scope':list(scope),'anomaly_score':score}]},history)['players'][0]
        for i in range(5):row=sample(1000+i*10)
        self.assertEqual(len(row['recent_windows']),5)
        self.assertEqual(len(sample(1100)['recent_windows']),1)
        self.assertEqual(len(sample(1110,('s','i','2','map'))['recent_windows']),1)
        sample(1120,score=None)
        self.assertEqual(history,{})
    def test_p99_requires_population_and_samples(self):
        self.assertIsNone(threshold([{'player':'p','score':.5}]*1000))
        self.assertIsNone(threshold([{'player':str(i%30),'score':.5} for i in range(1000)]))
        samples=[{'player':str(i%30),'score':i/1000} for i in range(1000)]
        self.assertEqual(threshold(samples),.99)

    def test_feed_health_and_match_isolation(self):
        now=10000
        event={'server_id':'s','instance_id':'i','match_row':1,'map':'internal',
            'ts':9999,'event_time':100,'event_id':'e'}
        live={'server_id':'s','ok':True,'feed_at':9999,'status_at':9999,
            'players_at':9999,'players':[{'steamId':'p'}]}
        data={'now':now,'live':[live],'events':[event],
            'matches':[{'server_id':'s','id':2}]}
        result,reference=evaluate(Model(),data,[])
        self.assertIsNone(result['players'][0]['ShortRisk'])
        data['matches'][0]['id']=1
        result,reference=evaluate(Model(),data,[])
        self.assertEqual(result['players'][0]['ShortRisk'],.9)
        self.assertFalse(result['players'][0]['kick_executed'])
        self.assertEqual(result['players'][0]['execution_status'],'handled_by_panel')
        self.assertEqual(len(reference),1)
        result,reference=evaluate(Model(),data,reference)
        self.assertEqual(len(reference),1)
        live['feed_at']=9000
        result,_=evaluate(Model(),data,reference)
        self.assertIsNone(result['players'][0]['ShortRisk'])

if __name__=='__main__':unittest.main()
