import { env } from '$env/dynamic/private';
import { z } from 'zod';
import type { QqProvider } from '$lib/qq-providers';

export const qqProviderSchema = z.enum(['napcat', 'llbot']);

export const policySchema = z.object({
	enabled: z.boolean().default(true),
	antiCheatNotices: z.boolean().default(true),
	serverId: z.string().min(1).max(100),
	groups: z.array(z.string().regex(/^[1-9]\d{4,15}$/)).min(1),
	lowAt: z.number().int().min(1).max(200).default(20),
	pointsPerMinute: z.number().int().min(1).max(100).default(1),
	voteCost: z.number().int().min(1).max(100000).default(10),
	broadcastCost: z.number().int().min(1).max(100000).default(20),
	reserveCost: z.number().int().min(1).max(100000).default(120),
	reserveHours: z.number().int().min(1).max(720).default(24),
	voteSeconds: z.number().int().min(30).max(240).default(120),
	maps: z.array(z.string().min(1).max(100)).min(2).max(10)
});
export type QqPolicy = z.infer<typeof policySchema>;
export function parsePolicies(raw: string): QqPolicy[] {
	const policies = z.array(policySchema).parse(JSON.parse(raw));
	const servers = policies.map((p) => p.serverId);
	const groups = policies.flatMap((p) => p.groups);
	if (new Set(servers).size !== servers.length || new Set(groups).size !== groups.length)
		throw new Error('QQ policy server IDs and group numbers must be unique.');
	for (const p of policies)
		if (new Set(p.maps).size !== p.maps.length) throw new Error('Duplicate QQ map.');
	return policies;
}
export interface QqConfiguration {
	provider: QqProvider;
	enabled: boolean;
	url: string;
	selfId: string;
	token: string;
	secret: string;
	policies: QqPolicy[];
}
let stored: QqConfiguration | null = null;
export function applyQqConfiguration(value: QqConfiguration | null) {
	stored = value;
}
export function environmentQqConfiguration(): QqConfiguration {
	return {
		provider: qqProviderSchema.parse(env.QQ_BOT_PROVIDER || 'napcat'),
		enabled: !!env.ONEBOT_HTTP_URL,
		url: env.ONEBOT_HTTP_URL || '',
		selfId: env.ONEBOT_SELF_ID || '',
		token: env.ONEBOT_ACCESS_TOKEN || '',
		secret: env.ONEBOT_EVENT_SECRET || '',
		policies: parsePolicies(env.QQ_BOT_POLICIES || '[]')
	};
}
export function validateQqConnection(urlValue: string, selfId: string) {
	let url: URL;
	try {
		url = new URL(urlValue);
	} catch {
		throw new Error('请输入完整的 QQ 机器人 HTTP 接口地址。');
	}
	if (
		!['http:', 'https:'].includes(url.protocol) ||
		url.username ||
		url.password ||
		url.search ||
		url.hash
	)
		throw new Error('接口地址只支持 HTTP/HTTPS，不能带账号、密码、查询参数或片段。');
	if (url.protocol === 'http:' && !['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname))
		throw new Error('远程 QQ 机器人连接必须使用 HTTPS；同机可使用回环 HTTP。');
	if (!/^[1-9]\d{4,15}$/.test(selfId)) throw new Error('请输入有效的机器人 QQ 号。');
	return url.toString().replace(/\/$/, '');
}
let cached = '';
let parsed: QqPolicy[] = [];
export function qqPolicies(): QqPolicy[] {
	if (stored) return stored.enabled ? stored.policies.filter((p) => p.enabled) : [];
	const raw = env.QQ_BOT_POLICIES || '[]';
	if (raw !== cached) {
		parsed = parsePolicies(raw);
		cached = raw;
	}
	return parsed.filter((p) => p.enabled);
}
export const qqPolicy = (serverId: string) => qqPolicies().find((p) => p.serverId === serverId);
export function qqCredentials() {
	if (stored)
		return stored.enabled
			? { url: stored.url, selfId: stored.selfId, token: stored.token, secret: stored.secret }
			: null;
	const values = [
		env.ONEBOT_HTTP_URL,
		env.ONEBOT_ACCESS_TOKEN,
		env.ONEBOT_EVENT_SECRET,
		env.ONEBOT_SELF_ID
	];
	if (!values.some(Boolean)) return null;
	if (!values.every(Boolean)) throw new Error('Complete all four ONEBOT settings.');
	return {
		url: validateQqConnection(env.ONEBOT_HTTP_URL!, env.ONEBOT_SELF_ID!),
		token: env.ONEBOT_ACCESS_TOKEN!,
		secret: env.ONEBOT_EVENT_SECRET!,
		selfId: env.ONEBOT_SELF_ID!
	};
}
