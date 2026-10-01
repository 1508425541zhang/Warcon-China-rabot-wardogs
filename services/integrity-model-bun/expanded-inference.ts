import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { Network } from './network';
import { buildExpanded, normalizeRows } from './expanded-features';
export class ExpandedPredictor {
	readonly manifest: any;
	readonly calibration: any;
	readonly contract: any;
	private network: Network;
	constructor(
		root: string,
		private sourceLoader?: (request: any) => Promise<any>
	) {
		this.manifest = JSON.parse(readFileSync(join(root, 'manifest.json'), 'utf8'));
		const load = (file: string, field: string) => {
			const data = readFileSync(join(root, file));
			if (createHash('sha256').update(data).digest('hex') !== this.manifest[field])
				throw Error('Artifact hash mismatch');
			return JSON.parse(data.toString());
		};
		this.contract = load('feature_scaler.json', 'scaler_sha256');
		this.calibration = load('calibration.json', 'calibration_sha256');
		if (
			this.manifest.channels !== this.contract.features.length * 2 ||
			this.manifest.window_steps !== 60 ||
			this.contract.window_steps !== 60
		)
			throw Error('Unsupported expanded model');
		this.network = new Network(
			root,
			this.manifest.weights_sha256,
			this.manifest.weights_index_sha256,
			60,
			this.manifest.channels
		);
	}
	scoreInput(input: Float32Array) {
		const pred = this.network.forward(input),
			f = this.contract.features.length,
			c = f * 2,
			totals = new Float64Array(f),
			counts = new Float64Array(f),
			pointScores: number[] = [];
		let total = 0,
			count = 0;
		const weights = this.contract.feature_groups.map((g: string) =>
			g === 'combat' ? 1 : g === 'weapon' ? 0.6 : g === 'economy' ? 0.3 : 0
		);
		for (let t = 0; t < 60; t++) {
			let a = 0,
				n = 0;
			for (let i = 0; i < f; i++)
				if (input[t * c + f + i] === 1 && weights[i] > 0) {
					const d = Math.abs(pred[t * c + i] - input[t * c + i]),
						h = d < 1 ? 0.5 * d * d : d - 0.5,
						e = h * weights[i];
					totals[i] += e;
					counts[i]++;
					a += e;
					n++;
				}
			pointScores.push(a / Math.max(n, 1));
			total += a;
			count += n;
		}
		const tails = Array.from(totals, (v, i) => v / Math.max(1, counts[i]))
			.sort((a, b) => b - a)
			.slice(0, 5);
		const score =
			(0.7 * total) / Math.max(1, count) + (0.3 * tails.reduce((a, b) => a + b, 0)) / tails.length;
		if (!Number.isFinite(score)) throw Error('Invalid score');
		return { score, pointScores };
	}
	async assess(request: any) {
		if (request?.schema !== this.manifest.feature_schema || typeof request?.requestId !== 'string')
			throw Error('Invalid request');
		const identity = {
			modelId: this.manifest.model_id,
			checkpointSha256: this.manifest.checkpoint_sha256,
			schema: this.manifest.feature_schema,
			requestId: request.requestId,
			stage: 'A测',
			calibrationSha256: this.manifest.calibration_sha256,
			windowSeconds: 1800,
			bucketSeconds: 30
		};
		if (!this.sourceLoader) throw Error('Live source reader unavailable');
		const source = await this.sourceLoader(request),
			rows = buildExpanded(source, this.contract),
			f = this.contract.features.length,
			input = normalizeRows(rows, this.contract),
			k = this.contract.features.indexOf('small_arm_kills_60s');
		const observed = rows.filter((r) => Number.isFinite(r[k])).length;
		if (rows.length !== 60 || observed < 48)
			return {
				...identity,
				status: 'INSUFFICIENT_DATA',
				reason: '需要同一对局连续30分钟序列及至少80%的步兵事件覆盖。',
				observedBuckets: observed,
				requiredBuckets: 60
			};
		return {
			...identity,
			status: 'READY',
			windowEnd: source.end,
			observedBuckets: observed,
			requiredBuckets: 60,
			featureCount: f,
			...this.scoreInput(input)
		};
	}
}
