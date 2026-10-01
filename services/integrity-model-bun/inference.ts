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
	constructor(root = join(import.meta.dir, 'artifacts-30m')) {
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
			![60, 200].includes(this.manifest.window_steps)
		)
			throw new Error('Unsupported model');
		this.network = new Network(
			root,
			this.manifest.weights_sha256,
			this.manifest.weights_index_sha256,
			this.manifest.window_steps
		);
	}
	score(rows: Row[]) {
		const steps = this.manifest.window_steps;
		if (rows.length !== steps) throw new Error(`Exactly ${steps} buckets required`);
		const x = new Float32Array(steps * 27);
		for (let t = 0; t < steps; t++) {
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
		const steps = this.manifest.window_steps;
		const rows = buildRows(request).slice(-steps);
		const end = Date.parse((request as any)?.sources?.matches?.[0]?.ended_at ?? '');
		const age = rows.length
			? end - Date.parse(String(rows.at(-1)!.bucket_start_utc)) - 30000
			: Infinity;
		const result = {
			modelId: this.manifest.model_id,
			checkpointSha256: this.manifest.checkpoint_sha256,
			schema: this.manifest.feature_schema,
			requestId: (request as Record<string, unknown>).requestId ?? null,
			stage: 'A测',
			calibrationSha256: this.manifest.calibration_sha256,
			windowSeconds: steps * 30,
			bucketSeconds: 30
		};
		const eligible =
			Number.isFinite(age) &&
			age >= 0 &&
			age <= 45000 &&
			rows.length === steps &&
			rows.every((r) => r.is_active === 1) &&
			rows.filter((r) => r.bucket_observed === 1).length >= steps * 0.8 &&
			rows.filter((r) => r.cash_observed === 1).length >= steps * 0.75;
		if (!eligible)
			return {
				...result,
				status: 'INSUFFICIENT_DATA',
				reason: `需要同局连续 ${steps / 2} 分钟有效序列、80% 观测及75%现金覆盖。`,
				observedBuckets: rows.filter((r) => r.bucket_observed === 1).length,
				requiredBuckets: steps
			};
		return {
			...result,
			status: 'READY',
			windowEnd: rows.at(-1)!.bucket_start_utc,
			...this.score(rows)
		};
	}
}
