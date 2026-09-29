import { env } from '$env/dynamic/private';
import { z } from 'zod';

const policySchema = z.object({
	serverId: z.string().min(1).max(100),
	groups: z.array(z.string().min(1).max(128)).min(1),
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
		throw new Error('QQ policy server IDs and group openids must be unique.');
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
	if (!env.QQ_BOT_APP_ID || !env.QQ_BOT_SECRET) return null;
	return { appId: env.QQ_BOT_APP_ID, secret: env.QQ_BOT_SECRET };
}
