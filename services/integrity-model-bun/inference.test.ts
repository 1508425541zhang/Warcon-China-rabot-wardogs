import { describe, expect, test } from 'bun:test';
import { Predictor } from './inference';
import { Network } from './network';
import { buildRows } from './features';
import { serve } from './server';
import parity from './parity.json';

const predictor = new Predictor();
function fixture(count = 205) {
	const stamp = (i: number) => new Date(Date.UTC(2026, 0, 1) + i * 30_000).toISOString();
	return {
		schema: 'warcon-raw-30s-v1',
		requestId: 'synthetic',
		sources: {
			matches: [{ id: 1, started_at: stamp(0), ended_at: stamp(count), map: 'synthetic' }],
			player_progress_samples: Array.from({ length: count }, (_, i) => ({
				server_id: 's',
				match_id: 1,
				observed_at: stamp(i),
				roster_size: 60,
				players: [
					{
						steamId: 'player',
						cash: 1000 + i * 20,
						kills: Math.floor(i / 20),
						deaths: Math.floor(i / 30)
					}
				]
			})),
			integrity_player_metric_history: [],
			integrity_windows: []
		}
	};
}
describe('native Epoch58 inference', () => {
	for (const reference of parity)
		test(`PyTorch parity: case ${reference.kind}`, () => {
			const input = new Float32Array(5400);
			for (let t = 0; t < 200; t++)
				for (let c = 0; c < 27; c++)
					input[t * 27 + c] =
						c >= 14
							? reference.kind === 1 && (t + c) % 3 === 0
								? 0
								: 1
							: Math.sin((t * 27 + c + 1) * 0.137) * (reference.kind === 2 ? 4 : 1);
			const n = new Network(
				import.meta.dir + '/artifacts',
				predictor.manifest.weights_sha256,
				predictor.manifest.weights_index_sha256
			);
			const pred = n.forward(input),
				result = n.score(input);
			expect(Math.abs(result.score - reference.score)).toBeLessThan(5e-6);
			for (const s of reference.predictionSamples)
				expect(Math.abs(pred[s.index] - s.value)).toBeLessThan(5e-5);
			for (let i = 0; i < 200; i++)
				expect(Math.abs(result.pointScores[i] - reference.pointScores[i])).toBeLessThan(5e-5);
		});
	test('raw-table reconstruction preserves missing values, roster and counter resets', () => {
		const request = fixture();
		request.sources.player_progress_samples[40].players[0].kills = 0;
		const rows = buildRows(request);
		expect(rows).toHaveLength(200);
		expect(rows[35].combat_change_valid).toBe(0);
		expect(rows.every((r) => r.roster_size === 60 && r.history_kpm_observed === 0)).toBe(true);
		expect(predictor.assess(request).status).toBe('READY');
	});
	test('short, offline gaps and missing cash cannot become normal scores', () => {
		expect(predictor.assess(fixture(50)).status).toBe('INSUFFICIENT_DATA');
		const missing = fixture();
		for (const r of missing.sources.player_progress_samples) r.players[0].cash = NaN;
		expect(predictor.assess(missing).status).toBe('INSUFFICIENT_DATA');
		const gap = fixture();
		gap.sources.player_progress_samples.splice(20, 20);
		expect(predictor.assess(gap).status).toBe('INSUFFICIENT_DATA');
	});
	test('schema mismatch and mixed players rejected', () => {
		expect(() => predictor.assess({ ...fixture(), schema: 'other' })).toThrow();
		const mixed = fixture();
		mixed.sources.player_progress_samples[20].players[0].steamId = 'other';
		expect(() => predictor.assess(mixed)).toThrow('One player');
	});
	test('authenticated HTTP health and inference', async () => {
		const token = 't'.repeat(32),
			server = serve(predictor, token, 0);
		try {
			const root = `http://127.0.0.1:${server.port}`,
				headers = { authorization: 'Bearer ' + token, 'content-type': 'application/json' };
			expect((await fetch(root + '/v1/health')).status).toBe(401);
			const health = (await (await fetch(root + '/v1/health', { headers })).json()) as any;
			expect(health.runtime).toBe('bun-native-float32');
			const response = await fetch(root + '/v1/assess', {
				method: 'POST',
				headers,
				body: JSON.stringify(fixture())
			});
			expect(response.status).toBe(200);
			expect(((await response.json()) as any).status).toBe('READY');
			expect(
				(await fetch(root + '/v1/assess', { method: 'POST', headers, body: '{' })).status
			).toBe(400);
		} finally {
			await server.stop(true);
		}
	});
});
