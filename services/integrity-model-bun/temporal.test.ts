import { expect, test } from 'bun:test';
import { Predictor } from './inference';
import { Network } from './network';
import parity from './artifacts-30m/parity.json';
const predictor = new Predictor();
function fixture(count = 65) {
	const stamp = (n: number) => new Date(Date.UTC(2026, 0, 1) + n * 30000).toISOString();
	return {
		schema: 'warcon-raw-30s-v1',
		requestId: 'temporal',
		sources: {
			matches: [{ id: 1, started_at: stamp(0), ended_at: stamp(count) }],
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
test('60-step adapted temporal network matches local PyTorch reference', () => {
	const network = new Network(
		import.meta.dir + '/artifacts-30m',
		predictor.manifest.weights_sha256,
		predictor.manifest.weights_index_sha256,
		60
	);
	for (const reference of parity) {
		const x = new Float32Array(60 * 27);
		for (let t = 0; t < 60; t++)
			for (let c = 0; c < 27; c++)
				x[t * 27 + c] =
					c >= 14
						? reference.kind === 1 && (t + c) % 3 === 0
							? 0
							: 1
						: Math.sin((t * 27 + c + 1) * 0.137) * (reference.kind === 2 ? 4 : 1);
		const result = network.score(x),
			prediction = network.forward(x);
		expect(Math.abs(result.score - reference.score)).toBeLessThan(5e-6);
		for (let i = 0; i < 60; i++)
			expect(Math.abs(result.pointScores[i] - reference.pointScores[i])).toBeLessThan(5e-5);
		for (const p of reference.predictionSamples)
			expect(Math.abs(prediction[p.index] - p.value)).toBeLessThan(5e-5);
	}
});
test('30-minute scoring retains order, never pads short windows and rejects stale data', () => {
	expect(predictor.assess(fixture()).status).toBe('READY');
	expect(predictor.assess(fixture(59)).status).toBe('INSUFFICIENT_DATA');
	const result = predictor.assess(fixture());
	expect('pointScores' in result && result.pointScores.length).toBe(60);
	expect(result.windowSeconds).toBe(1800);
	expect(result.bucketSeconds).toBe(30);
	const stale = fixture();
	stale.sources.matches[0].ended_at = new Date(Date.UTC(2026, 0, 1) + 8000000).toISOString();
	expect(predictor.assess(stale).status).toBe('INSUFFICIENT_DATA');
	const missing = fixture();
	for (const r of missing.sources.player_progress_samples) r.players[0].cash = NaN;
	expect(predictor.assess(missing).status).toBe('INSUFFICIENT_DATA');
	const gap = fixture();
	gap.sources.player_progress_samples.splice(30, 20);
	expect(predictor.assess(gap).status).toBe('INSUFFICIENT_DATA');
});
