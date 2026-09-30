import { eq, sql } from 'drizzle-orm';
import { randomUUID } from 'node:crypto';
import { z } from 'zod';
import type { Env } from '../env';
import { siteSettings } from '../db/schema';
import { encryptSecret, decryptSecret } from '../crypto';
import { ApiError } from '../http';
import calibration from '../../../../services/integrity-model-bun/artifacts/calibration.json';
import manifest from '../../../../services/integrity-model-bun/artifacts/manifest.json';
export const MODEL_CALIBRATION = calibration;
export const CALIBRATION_SHA = manifest.calibration_sha256;

export const MODEL_ID = 'warcon-at-epoch58';
export const MODEL_SHA = 'ea3bc3f168e7ca8e329e85736d74c59019d6392cbded31bc63bee41ad7378da7';
export const MODEL_SCHEMA = 'warcon-raw-30s-v1';
const key = (orgId: string) => `integrityModel:${orgId}`;
export type ModelConfig = {
	revision: string;
	developerEnabled: boolean;
	url: string;
	tokenEnc: string;
	autoPunishEnabled: boolean;
	maxActionsPerHour: number;
	cooldownSeconds: number;
	intervalSeconds: number;
};
type Config = ModelConfig;
const defaults: Config = {
	revision: 'empty',
	developerEnabled: false,
	url: '',
	tokenEnc: '',
	autoPunishEnabled: true,
	maxActionsPerHour: 10,
	cooldownSeconds: 600,
	intervalSeconds: 600
};
export async function modelConfig(env: Env, orgId: string): Promise<Config> {
	const [row] = await env.db
		.select()
		.from(siteSettings)
		.where(eq(siteSettings.key, key(orgId)));
	return row ? (row.value as Config) : { ...defaults };
}
export async function modelConfigView(env: Env, orgId: string) {
	const { tokenEnc, ...config } = await modelConfig(env, orgId);
	return {
		...config,
		hasToken: !!tokenEnc,
		modelId: MODEL_ID,
		stage: 'A测',
		p95: calibration.p95,
		p99: calibration.p99
	};
}
const configSchema = z.object({
	revision: z.string().max(100),
	developerEnabled: z.boolean(),
	url: z.string().max(2000),
	token: z.string().max(512).optional(),
	autoPunishEnabled: z.boolean().default(true),
	maxActionsPerHour: z.number().int().min(1).max(100).default(10),
	cooldownSeconds: z.number().int().min(60).max(86400).default(600),
	intervalSeconds: z.number().int().min(60).max(3600)
});
export function modelUrl(value: string) {
	let url: URL;
	try {
		url = new URL(value);
	} catch {
		throw new ApiError(400, '模型 API 地址无效。');
	}
	if (
		!['http:', 'https:'].includes(url.protocol) ||
		url.username ||
		url.password ||
		url.search ||
		url.hash ||
		!['', '/'].includes(url.pathname)
	)
		throw new ApiError(400, '填写 HTTP(S) 服务根地址，不包含路径、账号或查询参数。');
	return url.origin;
}
export async function saveModelConfig(env: Env, orgId: string, userId: string, input: unknown) {
	const parsed = configSchema.safeParse(input);
	if (!parsed.success) throw new ApiError(400, '模型配置无效。');
	const p = parsed.data;
	const url = p.url ? modelUrl(p.url) : '';
	if (p.token && (p.token.length < 32 || /\s/.test(p.token)))
		throw new ApiError(400, '令牌至少 32 个字符且不包含空白。');
	await env.db.transaction(async (tx) => {
		await tx.execute(sql`SELECT pg_advisory_xact_lock(hashtextextended(${key(orgId)},0))`);
		const [row] = await tx
			.select()
			.from(siteSettings)
			.where(eq(siteSettings.key, key(orgId)));
		const old = row ? (row.value as Config) : defaults;
		if (old.revision !== p.revision) throw new ApiError(409, '配置已变更，请重新加载。');
		const tokenEnc = p.token ? encryptSecret(env, p.token) : old.tokenEnc;
		if (p.developerEnabled && (!url || !tokenEnc))
			throw new ApiError(400, '开发者模式需要 API 地址和令牌。');
		const value: Config = {
			revision: randomUUID(),
			developerEnabled: p.developerEnabled,
			url,
			tokenEnc,
			autoPunishEnabled: p.autoPunishEnabled,
			maxActionsPerHour: p.maxActionsPerHour,
			cooldownSeconds: p.cooldownSeconds,
			intervalSeconds: p.intervalSeconds
		};
		await tx
			.insert(siteSettings)
			.values({ key: key(orgId), value, updatedBy: userId, updatedAt: new Date() })
			.onConflictDoUpdate({
				target: siteSettings.key,
				set: { value, updatedBy: userId, updatedAt: new Date() }
			});
	});
	return modelConfigView(env, orgId);
}
export async function callModel(env: Env, config: Config, body?: unknown) {
	if (!config.developerEnabled || !config.tokenEnc)
		throw new ApiError(409, '模型开发者模式未启用。');
	const data = body === undefined ? undefined : JSON.stringify(body);
	if (data && Buffer.byteLength(data) > 8 * 1024 * 1024)
		throw new ApiError(413, '模型输入超限，未截断数据。');
	try {
		const response = await fetch(`${modelUrl(config.url)}/v1/${data ? 'assess' : 'health'}`, {
			method: data ? 'POST' : 'GET',
			headers: {
				authorization: `Bearer ${decryptSecret(env, config.tokenEnc)}`,
				'content-type': 'application/json'
			},
			body: data,
			redirect: 'error',
			signal: AbortSignal.timeout(30000)
		});
		if (!response.ok) throw new Error('HTTP error');
		const reader = response.body?.getReader();
		if (!reader) throw new Error('Empty response');
		let size = 0;
		const chunks: Uint8Array[] = [];
		while (true) {
			const part = await reader.read();
			if (part.done) break;
			size += part.value.length;
			if (size > 65536) {
				await reader.cancel();
				throw new Error('Response too large');
			}
			chunks.push(part.value);
		}
		return JSON.parse(Buffer.concat(chunks).toString('utf8')) as Record<string, unknown>;
	} catch {
		throw new ApiError(502, '模型 API 请求失败、超时或返回无效数据。');
	}
}
export async function testModel(env: Env, orgId: string) {
	const result = await callModel(env, await modelConfig(env, orgId));
	if (
		result.model_id !== MODEL_ID ||
		result.checkpoint_sha256 !== MODEL_SHA ||
		result.feature_schema !== MODEL_SCHEMA ||
		result.calibration_sha256 !== CALIBRATION_SHA
	)
		throw new ApiError(502, '模型版本、权重或输入格式不匹配。');
	return { modelId: MODEL_ID, stage: 'A测', message: 'HTTP 连接及模型指纹验证通过。' };
}
export function validateModelResult(result: Record<string, unknown>, requestId: string) {
	if (
		result.modelId !== MODEL_ID ||
		result.checkpointSha256 !== MODEL_SHA ||
		result.schema !== MODEL_SCHEMA ||
		result.requestId !== requestId ||
		result.calibrationSha256 !== CALIBRATION_SHA
	)
		throw new ApiError(502, '模型响应身份不匹配。');
	if (result.status === 'INSUFFICIENT_DATA')
		return { status: 'INSUFFICIENT_DATA' as const, score: null, detail: result };
	if (
		result.status !== 'READY' ||
		typeof result.score !== 'number' ||
		!Number.isFinite(result.score) ||
		result.score < 0 ||
		!Array.isArray(result.pointScores) ||
		result.pointScores.length !== 200 ||
		result.pointScores.some((x) => typeof x !== 'number' || !Number.isFinite(x) || x < 0)
	)
		throw new ApiError(502, '模型评分格式无效。');
	return { status: 'READY' as const, score: result.score, detail: result };
}
