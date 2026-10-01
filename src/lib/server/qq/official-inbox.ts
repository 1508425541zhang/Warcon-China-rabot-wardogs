import { sql } from 'drizzle-orm';
import type { Env } from '../env';
import { qqCredentials, qqPolicies } from './config';
import { officialMessage } from './official';
export async function acceptOfficialEvent(env: Env, payload: unknown, appId: string) {
	const c = qqCredentials();
	if (c?.provider !== 'official' || c.selfId !== appId) return;
	const message = officialMessage(payload, appId);
	if (!message) return;
	const policy = qqPolicies().find(p => p.groups.includes(message.groupId));
	if (!policy) {
		// Group OpenID is useful for configuration; never log message text or member identifiers.
		console.info('[qq-official] unconfigured group OpenID=' + message.groupId); return;
	}
	await env.db.transaction(async tx => {
		await tx.execute(sql`SELECT pg_advisory_xact_lock(hashtextextended(${`qq-event:${message.memberId}`},0))`);
		const [n] = await tx.execute<{n:string}>(sql`SELECT count(*) AS n FROM qq_inbox WHERE member_id=${message.memberId} AND created_at>now()-interval '1 minute'`);
		if (Number(n.n) >= 10) return;
		await tx.execute(sql`INSERT INTO qq_inbox(id,server_id,group_id,member_id,content) VALUES(${message.id},${policy.serverId},${message.groupId},${message.memberId},${message.content}) ON CONFLICT DO NOTHING`);
	});
}
