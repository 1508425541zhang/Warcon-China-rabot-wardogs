import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

type Tensor = { shape: number[]; offset: number; length: number };
const steps = 200,
	width = 128;
const sum = (x: Float32Array) => x.reduce((a, b) => a + b, 0);
const f32 = Math.fround;

// Error-function GELU, matching PyTorch approximate='none', not tanh GELU.
function gelu(x: number) {
	const z = Math.abs(x) / Math.SQRT2,
		t = 1 / (1 + 0.3275911 * z);
	const erf =
		1 -
		((((1.061405429 * t - 1.453152027) * t + 1.421413741) * t - 0.284496736) * t + 0.254829592) *
			t *
			Math.exp(-z * z);
	return 0.5 * x * (1 + (x < 0 ? -erf : erf));
}

export class Network {
	private weights: Float32Array;
	private index: Record<string, Tensor>;
	constructor(root: string, weightsHash: string, indexHash: string) {
		const raw = readFileSync(join(root, 'weights.f32'));
		const metadata = readFileSync(join(root, 'weights.json'));
		for (const [bytes, hash] of [
			[raw, weightsHash],
			[metadata, indexHash]
		] as const)
			if (createHash('sha256').update(bytes).digest('hex') !== hash)
				throw new Error('Model artifact hash mismatch');
		const copy = new Uint8Array(raw.length);
		copy.set(raw);
		this.weights = new Float32Array(copy.buffer);
		this.index = JSON.parse(metadata.toString()).tensors;
		for (const tensor of Object.values(this.index))
			if (
				tensor.offset < 0 ||
				tensor.length !== tensor.shape.reduce((a, b) => a * b, 1) ||
				tensor.offset + tensor.length > this.weights.length
			)
				throw new Error('Invalid tensor bounds');
	}
	private tensor(name: string, shape: number[]) {
		const t = this.index[name];
		if (!t || JSON.stringify(t.shape) !== JSON.stringify(shape))
			throw new Error(`Invalid tensor: ${name}`);
		return this.weights.subarray(t.offset, t.offset + t.length);
	}
	private linear(x: Float32Array, input: number, output: number, prefix: string, conv = false) {
		const w = this.tensor(prefix + '.weight', conv ? [output, input, 1] : [output, input]);
		const b = this.tensor(prefix + '.bias', [output]);
		const y = new Float32Array(steps * output);
		for (let t = 0; t < steps; t++)
			for (let o = 0; o < output; o++) {
				let value = b[o];
				const a = t * input,
					k = o * input;
				for (let i = 0; i < input; i++) value += x[a + i] * w[k + i];
				y[t * output + o] = value;
			}
		return y;
	}
	private norm(x: Float32Array, prefix: string) {
		const w = this.tensor(prefix + '.weight', [width]),
			b = this.tensor(prefix + '.bias', [width]);
		const y = new Float32Array(x.length);
		for (let t = 0; t < steps; t++) {
			const start = t * width;
			let mean = 0,
				variance = 0;
			for (let i = 0; i < width; i++) mean += x[start + i];
			mean /= width;
			for (let i = 0; i < width; i++) variance += (x[start + i] - mean) ** 2;
			const scale = 1 / Math.sqrt(variance / width + 1e-5);
			for (let i = 0; i < width; i++) y[start + i] = (x[start + i] - mean) * scale * w[i] + b[i];
		}
		return y;
	}
	private attention(x: Float32Array, prefix: string) {
		const q = this.linear(x, width, width, prefix + '.query_projection');
		const k = this.linear(x, width, width, prefix + '.key_projection');
		const v = this.linear(x, width, width, prefix + '.value_projection');
		const y = new Float32Array(x.length),
			logits = new Float32Array(steps);
		for (let h = 0; h < 4; h++)
			for (let t = 0; t < steps; t++) {
				let max = -Infinity;
				for (let s = 0; s < steps; s++) {
					let dot = 0;
					for (let d = 0; d < 32; d++) dot += q[t * width + h * 32 + d] * k[s * width + h * 32 + d];
					logits[s] = f32(dot) / Math.sqrt(32);
					max = Math.max(max, logits[s]);
				}
				let denominator = 0;
				for (let s = 0; s < steps; s++) {
					logits[s] = Math.exp(logits[s] - max);
					denominator += logits[s];
				}
				for (let s = 0; s < steps; s++) logits[s] /= denominator;
				for (let d = 0; d < 32; d++) {
					let value = 0;
					for (let s = 0; s < steps; s++) value += logits[s] * v[s * width + h * 32 + d];
					y[t * width + h * 32 + d] = value;
				}
			}
		// Sigma/prior outputs are training-only; the deployed reconstruction score does not use them.
		return this.linear(y, width, width, prefix + '.out_projection');
	}
	forward(input: Float32Array) {
		if (input.length !== steps * 27 || !input.every(Number.isFinite))
			throw new Error('Invalid model input');
		const conv = this.tensor('embedding.value_embedding.tokenConv.weight', [width, 27, 3]);
		const pe = this.tensor('embedding.position_embedding.pe', [1, steps, width]);
		let x = new Float32Array(steps * width);
		for (let t = 0; t < steps; t++)
			for (let o = 0; o < width; o++) {
				let value = 0;
				for (let i = 0; i < 27; i++)
					for (let k = 0; k < 3; k++)
						value += input[((t + k - 1 + steps) % steps) * 27 + i] * conv[(o * 27 + i) * 3 + k];
				x[t * width + o] = f32(value) + pe[t * width + o];
			}
		for (let layer = 0; layer < 2; layer++) {
			const prefix = `encoder.attn_layers.${layer}`;
			const a = this.attention(x, prefix + '.attention');
			for (let i = 0; i < x.length; i++) x[i] += a[i];
			x = this.norm(x, prefix + '.norm1');
			const ff = this.linear(x, width, 256, prefix + '.conv1', true);
			for (let i = 0; i < ff.length; i++) ff[i] = gelu(ff[i]);
			const out = this.linear(ff, 256, width, prefix + '.conv2', true);
			for (let i = 0; i < x.length; i++) x[i] += out[i];
			x = this.norm(x, prefix + '.norm2');
		}
		return this.linear(this.norm(x, 'encoder.norm'), width, 27, 'projection');
	}
	score(input: Float32Array) {
		const pred = this.forward(input),
			map = [0, 1, 2, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
		const points = new Float32Array(steps),
			numeric = new Float32Array(steps),
			masks = new Float32Array(steps),
			counts = new Float32Array(steps);
		for (let t = 0; t < steps; t++) {
			for (let i = 0; i < 14; i++) {
				const observed = input[t * 27 + 14 + map[i]];
				numeric[t] += f32(f32(pred[t * 27 + i] - input[t * 27 + i]) ** 2) * observed;
				counts[t] += observed;
			}
			for (let i = 14; i < 27; i++) masks[t] += f32(f32(pred[t * 27 + i] - input[t * 27 + i]) ** 2);
			points[t] = (0.8 * numeric[t]) / Math.max(1, counts[t]) + (0.2 * masks[t]) / 13;
		}
		const score = f32(
			(0.8 * sum(numeric)) / Math.max(1, sum(counts)) + (0.2 * sum(masks)) / (steps * 13)
		);
		if (!Number.isFinite(score) || !points.every(Number.isFinite))
			throw new Error('Nonfinite model output');
		return { score, pointScores: Array.from(points) };
	}
}
