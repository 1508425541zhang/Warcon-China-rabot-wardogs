import copy
import datetime as dt
import json
import threading
import unittest
import urllib.request
import urllib.error
from http.server import HTTPServer

from inference import Predictor
from server import make_handler


def fixture(count=205):
    start = dt.datetime(2026, 1, 1, tzinfo=dt.timezone.utc)
    iso = lambda seconds: (start + dt.timedelta(seconds=seconds)).isoformat()
    return {'schema': 'warcon-raw-30s-v1', 'requestId': 'synthetic', 'sources': {
        'matches': [{'id': 1, 'started_at': iso(0), 'ended_at': iso(count * 30), 'map': 'synthetic'}],
        'player_progress_samples': [{'server_id': 's', 'match_id': 1, 'observed_at': iso(i * 30),
            'roster_size': 60, 'players': [{'steamId': 'player', 'cash': 1000 + i * 20, 'kills': i // 20, 'deaths': i // 30}]}
            for i in range(count)],
        'integrity_player_metric_history': [], 'integrity_windows': []}}


class InferenceTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.predictor = Predictor()

    def test_real_checkpoint_and_http_roundtrip(self):
        server = HTTPServer(('127.0.0.1', 0), make_handler(self.predictor, 'a' * 32))
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        root = f'http://127.0.0.1:{server.server_port}'
        try:
            with self.assertRaises(urllib.error.HTTPError) as caught:
                urllib.request.urlopen(root + '/v1/health')
            self.assertEqual(caught.exception.code, 401)
            req = urllib.request.Request(root + '/v1/assess', data=json.dumps(fixture()).encode(),
                headers={'Authorization': 'Bearer ' + 'a' * 32, 'Content-Type': 'application/json'})
            with urllib.request.urlopen(req) as response:
                result = json.load(response)
            self.assertEqual(result['status'], 'READY')
            self.assertEqual(len(result['pointScores']), 200)
            self.assertGreaterEqual(result['score'], 0)
            self.assertEqual(result['checkpointSha256'], self.predictor.manifest['checkpoint_sha256'])
            self.assertEqual(result, self.predictor.assess(fixture()))
        finally:
            server.shutdown()
            server.server_close()
            thread.join()

    def test_missing_and_short_data_are_not_normal(self):
        self.assertEqual(self.predictor.assess(fixture(50))['status'], 'INSUFFICIENT_DATA')
        request = fixture()
        for row in request['sources']['player_progress_samples']:
            row['players'][0]['cash'] = None
        self.assertEqual(self.predictor.assess(request)['status'], 'INSUFFICIENT_DATA')

    def test_schema_rejected(self):
        request = fixture()
        request['schema'] = 'other'
        with self.assertRaises(ValueError):
            self.predictor.assess(request)

    def test_roster_count_survives_player_filtering(self):
        original = self.predictor.score
        captured = []
        def capture(rows):
            captured.extend(rows)
            return original(rows)
        self.predictor.score = capture
        try:
            self.predictor.assess(fixture())
            self.assertTrue(all(row['roster_size'] == '60' for row in captured))
            self.assertTrue(all(row['history_kpm_observed'] == '0' for row in captured))
        finally:
            self.predictor.score = original


if __name__ == '__main__':
    unittest.main()
