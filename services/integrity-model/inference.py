"""Epoch 58 inference, using the original training feature builder and frozen scaler."""
import contextlib
import csv
import hashlib
import io
import json
import tempfile
from pathlib import Path

import numpy as np
import torch

import features
from model.AnomalyTransformer import AnomalyTransformer

ROOT = Path(__file__).resolve().parent
MASK_FOR = [0, 1, 2, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]


class Predictor:
    def __init__(self):
        self.manifest = json.loads((ROOT / 'artifacts/manifest.json').read_text(encoding='utf-8'))
        for filename, field in [(self.manifest['checkpoint'], 'checkpoint_sha256'),
                                ('feature_scaler.json', 'scaler_sha256'), ('calibration.json', 'calibration_sha256')]:
            if hashlib.sha256((ROOT / 'artifacts' / filename).read_bytes()).hexdigest() != self.manifest[field]:
                raise RuntimeError('Artifact hash mismatch')
        self.scaler = json.loads((ROOT / 'artifacts/feature_scaler.json').read_text())
        self.calibration = json.loads((ROOT / 'artifacts/calibration.json').read_text(encoding='utf-8'))
        torch.set_num_threads(2)
        self.model = AnomalyTransformer(win_size=200, enc_in=27, c_out=27, d_model=128,
            n_heads=4, e_layers=2, d_ff=256, dropout=0.1, activation='gelu', output_attention=True)
        saved = torch.load(ROOT / 'artifacts' / self.manifest['checkpoint'], map_location='cpu', weights_only=True)
        if saved['epoch'] != self.manifest['epoch']:
            raise RuntimeError('Checkpoint epoch mismatch')
        self.model.load_state_dict(saved['state_dict'], strict=True)
        self.model.eval()

    def score(self, rows):
        if len(rows) != 200:
            raise ValueError('Exactly 200 consecutive 30-second buckets required')
        x = np.zeros((1, 200, 27), dtype=np.float32)
        for t, row in enumerate(rows):
            for i, name in enumerate(self.scaler['channel_order'][:14]):
                if float(row[features.MASKS[MASK_FOR[i]]]) == 1:
                    value = float(row[name])
                    scale = self.scaler['continuous_channels'][name]
                    x[0, t, i] = (value - scale['mean']) / scale['std']
            x[0, t, 14:] = [float(row[m]) for m in features.MASKS]
        if not np.isfinite(x).all():
            raise ValueError('Nonfinite features')
        with torch.inference_mode():
            target = torch.from_numpy(x)
            pred, _, _, _ = self.model(target)
            mask = target[..., [14 + m for m in MASK_FOR]]
            error = (pred[..., :14] - target[..., :14]).square() * mask
            point = .8 * error.sum(-1) / mask.sum(-1).clamp_min(1) + .2 * (pred[..., 14:] - target[..., 14:]).square().mean(-1)
            loss = .8 * error.sum() / mask.sum().clamp_min(1) + .2 * (pred[..., 14:] - target[..., 14:]).square().mean()
        if not torch.isfinite(point).all() or not torch.isfinite(loss):
            raise ValueError('Nonfinite model output')
        return {'score': loss.item(), 'pointScores': point[0].tolist()}

    def assess(self, request):
        if request.get('schema') != self.manifest['feature_schema']:
            raise ValueError('Feature schema mismatch')
        sources = request.get('sources', {})
        required = ['matches', 'player_progress_samples', 'integrity_player_metric_history', 'integrity_windows']
        if set(sources) != set(required) or any(not isinstance(sources[k], list) for k in required):
            raise ValueError('Invalid source tables')
        if len(sources['matches']) != 1 or any(len(sources[k]) > 25000 for k in required):
            raise ValueError('One match and bounded sources required')
        with tempfile.TemporaryDirectory(prefix='warcon-model-') as tmp:
            root = Path(tmp)
            (root / 'related_tables').mkdir()
            for name in required + ['kills']:
                (root / 'related_tables' / (name + '.jsonl')).write_text(
                    ''.join(json.dumps(r, allow_nan=False) + '\n' for r in sources.get(name, [])), encoding='utf-8')
            for name in ['training_observations', 'training_feed_batches']:
                (root / (name + '.jsonl')).write_text('', encoding='utf-8')
            with contextlib.redirect_stdout(io.StringIO()):
                features.build(root, root / 'derived', bucket_seconds=30)
            with (root / 'derived/complete_time_buckets.csv').open(encoding='utf-8', newline='') as f:
                rows = list(csv.DictReader(f))
        groups = {}
        for row in rows:
            # Original CSV omitted this mask; training reconstructed it from field presence.
            row['active_observed'] = '1' if row.get('active_fraction') not in ('', None) else '0'
            groups.setdefault(row['player_key'], []).append(row)
        if len(groups) > 1:
            raise ValueError('One player per request required')
        rows = next(iter(groups.values()), [])[-200:]
        result = {'modelId': self.manifest['model_id'], 'checkpointSha256': self.manifest['checkpoint_sha256'],
                  'schema': self.manifest['feature_schema'], 'requestId': request.get('requestId'), 'stage': 'A测',
                  'calibrationSha256': self.manifest['calibration_sha256']}
        eligible = (len(rows) == 200 and all(r['is_active'] == '1' for r in rows)
                    and sum(r['bucket_observed'] == '1' for r in rows) >= 160
                    and sum(r['cash_observed'] == '1' for r in rows) >= 150)
        if not eligible:
            return {**result, 'status': 'INSUFFICIENT_DATA', 'reason': 'Requires 100 minutes, active throughout, 80% observed and 75% cash coverage.'}
        return {**result, 'status': 'READY', 'windowEnd': rows[-1]['bucket_start_utc'], **self.score(rows)}
