import { env } from '$env/dynamic/private';
import { z } from 'zod';

const policySchema = z.object({
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
let cached = '';
let parsed: QqPolicy[] = [];
export function qqPolicies(): QqPolicy[] {
	const raw = env.QQ_BOT_POLICIES || '[]';
	if (raw !== cached) {
		parsed = parsePolicies(raw);
		cached = raw;
	}
	return parsed;
}
export const qqPolicy = (serverId: string) => qqPolicies().find((p) => p.serverId === serverId);
export function qqCredentials() {
	const values = [
		env.ONEBOT_HTTP_URL,
		env.ONEBOT_ACCESS_TOKEN,
		env.ONEBOT_EVENT_SECRET,
		env.ONEBOT_SELF_ID
	];
	if (!values.some(Boolean)) return null;
	if (!values.every(Boolean)) throw new Error('Complete all four ONEBOT settings.');
	const url = new URL(env.ONEBOT_HTTP_URL!);
	if (
		!['http:', 'https:'].includes(url.protocol) ||
		url.username ||
		url.password ||
		url.search ||
		url.hash
	)
		throw new Error('Invalid ONEBOT_HTTP_URL.');
	if (url.protocol === 'http:' && !['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname))
		throw new Error('Remote OneBot connections require HTTPS.');
	if (!/^[1-9]\d{4,15}$/.test(env.ONEBOT_SELF_ID!)) throw new Error('Invalid ONEBOT_SELF_ID.');
	return {
		url: url.toString().replace(/\/$/, ''),
		token: env.ONEBOT_ACCESS_TOKEN!,
		secret: env.ONEBOT_EVENT_SECRET!,
		selfId: env.ONEBOT_SELF_ID!
	};
}
