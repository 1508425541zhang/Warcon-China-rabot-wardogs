import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { buildRows, masks, maskFor, type Row } from './features';
import { Network } from './network';

export class Predictor {
	readonly manifest: Record<string, any>;
	readonly calibration: { p95: number; p99: number };
	private scaler: {
		channel_order: string[];
		continuous_channels: Record<string, { mean: number; std: number }>;
	};
	private network: Network;
	constructor(root = join(import.meta.dir, 'artifacts')) {
		this.manifest = JSON.parse(readFileSync(join(root, 'manifest.json'), 'utf8'));
		const load = (file: string, field: string) => {
			const data = readFileSync(join(root, file));
			if (createHash('sha256').update(data).digest('hex') !== this.manifest[field])
				throw new Error('Artifact hash mismatch');
			return JSON.parse(data.toString());
		};
		this.scaler = load('feature_scaler.json', 'scaler_sha256');
		this.calibration = load('calibration.json', 'calibration_sha256');
		if (
			this.manifest.epoch !== 58 ||
			this.scaler.channel_order.length !== 27 ||
			this.manifest.window_steps !== 200
		)
			throw new Error('Unsupported model');
		this.network = new Network(
			root,
			this.manifest.weights_sha256,
			this.manifest.weights_index_sha256
		);
	}
	score(rows: Row[]) {
		if (rows.length !== 200) throw new Error('Exactly 200 buckets required');
		const x = new Float32Array(200 * 27);
		for (let t = 0; t < 200; t++) {
			for (let i = 0; i < 14; i++)
				if (Number(rows[t][masks[maskFor[i]]]) === 1) {
					const name = this.scaler.channel_order[i],
						scale = this.scaler.continuous_channels[name];
					if (
						rows[t][name] === null ||
						rows[t][name] === undefined ||
						!Number.isFinite(Number(rows[t][name])) ||
						!(scale.std > 0)
					)
						throw new Error('Invalid feature value');
					x[t * 27 + i] = (Number(rows[t][name]) - scale.mean) / scale.std;
				}
			for (let i = 0; i < 13; i++) {
				const mask = Number(rows[t][masks[i]]);
				if (mask !== 0 && mask !== 1) throw new Error('Invalid feature mask');
				x[t * 27 + 14 + i] = mask;
			}
		}
		return this.network.score(x);
	}
	assess(request: unknown) {
		const rows = buildRows(request);
		const result = {
			modelId: this.manifest.model_id,
			checkpointSha256: this.manifest.checkpoint_sha256,
			schema: this.manifest.feature_schema,
			requestId: (request as Record<string, unknown>).requestId ?? null,
			stage: 'A测',
			calibrationSha256: this.manifest.calibration_sha256
		};
		const eligible =
			rows.length === 200 &&
			rows.every((r) => r.is_active === 1) &&
			rows.filter((r) => r.bucket_observed === 1).length >= 160 &&
			rows.filter((r) => r.cash_observed === 1).length >= 150;
		if (!eligible)
			return {
				...result,
				status: 'INSUFFICIENT_DATA',
				reason: 'Requires 100 minutes, active throughout, 80% observed and 75% cash coverage.'
			};
		return {
			...result,
			status: 'READY',
			windowEnd: rows.at(-1)!.bucket_start_utc,
			...this.score(rows)
		};
	}
}
