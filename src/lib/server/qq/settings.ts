import { eq, sql } from 'drizzle-orm';
import { randomUUID } from 'node:crypto';
import { z } from 'zod';
import type { Env } from '../env';
import { siteSettings, servers } from '../db/schema';
import { encryptSecret, decryptSecret } from '../crypto';
import { ApiError } from '../http';
import { qqProviders } from '$lib/qq-providers';
import {
	applyQqConfiguration,
	environmentQqConfiguration,
	parsePolicies,
	policySchema,
	qqProviderSchema,
	validateQqConnection,
	type QqConfiguration
} from './config';

const KEY = 'qqCommunity';
type Stored = Omit<QqConfiguration, 'token' | 'secret'> & {
	revision: string;
	tokenEnc: string;
	secretEnc: string;
};
function decode(env: Env, value: Stored): QqConfiguration {
	return {
		...value,
		provider: qqProviderSchema.parse(value.provider || 'napcat'),
		policies: parsePolicies(JSON.stringify(value.policies)),
		token: value.tokenEnc ? decryptSecret(env, value.tokenEnc) : '',
		secret: value.secretEnc ? decryptSecret(env, value.secretEnc) : ''
	};
}
export async function loadQqSettings(env: Env) {
	const [row] = await env.db.select().from(siteSettings).where(eq(siteSettings.key, KEY));
	applyQqConfiguration(row ? decode(env, row.value as Stored) : null);
}
export async function qqSettingsView(env: Env) {
	const [row] = await env.db.select().from(siteSettings).where(eq(siteSettings.key, KEY));
	const c = row ? decode(env, row.value as Stored) : environmentQqConfiguration();
	return {
		revision: row ? (row.value as Stored).revision : 'environment',
		source: row ? 'database' : 'environment',
		provider: c.provider,
		enabled: c.enabled,
		url: c.url,
		selfId: c.selfId,
		hasToken: !!c.token,
		hasSecret: !!c.secret,
		policies: c.policies,
		updatedAt: row?.updatedAt.toISOString() || null
	};
}
const patchSchema = z.object({
	revision: z.string().min(1).max(100),
	provider: qqProviderSchema.optional(),
	enabled: z.boolean(),
	url: z.string().trim().max(2000),
	selfId: z.string().trim().max(16),
	token: z.string().max(512).optional(),
	secret: z.string().max(512).optional(),
	clearToken: z.boolean().optional(),
	clearSecret: z.boolean().optional(),
	policies: z.array(policySchema).max(100)
});
export async function saveQqSettings(env: Env, input: unknown, userId: string) {
	const parsed = patchSchema.safeParse(input);
	if (!parsed.success)
		throw new ApiError(400, '配置格式不正确，请检查 QQ 号、群号、候选地图和各项数值范围。');
	const patch = parsed.data;
	let policies;
	try {
		policies = parsePolicies(JSON.stringify(patch.policies));
	} catch {
		throw new ApiError(400, '服务器、群号和候选地图不能重复。');
	}
	for (const secret of [patch.token, patch.secret])
		if (secret && (secret.length < 16 || /[\s\x00-\x1f]/.test(secret)))
			throw new ApiError(400, '密钥至少 16 个字符，且不能包含空白或控制字符。');
	if (patch.url || patch.selfId || patch.enabled) {
		try {
			patch.url = validateQqConnection(patch.url, patch.selfId);
		} catch (e) {
			throw new ApiError(400, (e as Error).message);
		}
	}
	await env.db.transaction(async (tx) => {
		await tx.execute(sql`SELECT pg_advisory_xact_lock(hashtextextended('qq-settings',0))`);
		const [row] = await tx.select().from(siteSettings).where(eq(siteSettings.key, KEY));
		if (patch.revision !== (row ? (row.value as Stored).revision : 'environment'))
			throw new ApiError(409, '配置已被其他管理员修改。请重新加载后再保存。');
		const previous = row ? decode(env, row.value as Stored) : environmentQqConfiguration();
		const token = patch.clearToken ? '' : patch.token || previous.token;
		const secret = patch.clearSecret ? '' : patch.secret || previous.secret;
		if (patch.enabled && (!token || !secret)) throw new ApiError(400, '启用前请填写两项密钥。');
		if (patch.enabled && !policies.some((p) => p.enabled))
			throw new ApiError(400, '启用前请至少添加一项启用的服务器规则。');
		const valid = new Set((await tx.select({ id: servers.id }).from(servers)).map((s) => s.id));
		if (policies.some((p) => !valid.has(p.serverId)))
			throw new ApiError(400, '规则中的服务器不存在，请重新选择。');
		const value: Stored = {
			revision: randomUUID(),
			provider: patch.provider ?? previous.provider,
			enabled: patch.enabled,
			url: patch.url,
			selfId: patch.selfId,
			policies,
			tokenEnc: token ? encryptSecret(env, token) : '',
			secretEnc: secret ? encryptSecret(env, secret) : ''
		};
		await tx
			.insert(siteSettings)
			.values({ key: KEY, value, updatedBy: userId, updatedAt: new Date() })
			.onConflictDoUpdate({
				target: siteSettings.key,
				set: { value, updatedBy: userId, updatedAt: new Date() }
			});
	});
	await loadQqSettings(env);
	return qqSettingsView(env);
}
/** Read-only probe; uses saved secrets and never posts to a group. */
export async function testQqConnection(env: Env, fetcher: typeof fetch = fetch) {
	const [row] = await env.db.select().from(siteSettings).where(eq(siteSettings.key, KEY));
	const c = row ? decode(env, row.value as Stored) : environmentQqConfiguration();
	if (!c.url || !c.selfId || !c.token)
		throw new ApiError(400, '请先保存连接地址、QQ 号和 API 密钥。');
	const url = validateQqConnection(c.url, c.selfId);
	try {
		const response = await fetcher(`${url}/get_login_info`, {
			method: 'POST',
			headers: { 'content-type': 'application/json', authorization: `Bearer ${c.token}` },
			body: '{}',
			redirect: 'error',
			signal: AbortSignal.timeout(10000)
		});
		if (!response.ok) throw new Error();
		const result = await response.json();
		if (result.status !== 'ok' || result.retcode !== 0 || String(result.data?.user_id) !== c.selfId)
			throw new Error();
		return {
			ok: true,
			selfId: c.selfId,
			message: `${qqProviders[c.provider].name} 接口已连接，登录 QQ 号匹配。群事件上报仍需在群中发送 /帮助 验证。`
		};
	} catch {
		throw new ApiError(
			502,
			`连接失败或登录 QQ 号不匹配，请检查 ${qqProviders[c.provider].name} 登录状态、接口地址和密钥。`
		);
	}
}
