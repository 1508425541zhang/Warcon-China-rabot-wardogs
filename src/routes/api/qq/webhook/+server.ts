import { sql } from 'drizzle-orm';
import { getEnv } from '$lib/server/env';
import { ApiError, route } from '$lib/server/http';
import { qqCredentials, qqPolicies } from '$lib/server/qq/config';
import { loadQqSettings } from '$lib/server/qq/settings';
import { oneBotMessage, verifyQq } from '$lib/server/qq/protocol';
import { verifyOfficial, officialValidation } from '$lib/server/qq/official';
import { acceptOfficialEvent } from '$lib/server/qq/official-inbox';

export const POST = route(async ({ request }) => {
	await loadQqSettings(getEnv());
	const credentials = qqCredentials();
	if (!credentials) throw new ApiError(404, 'QQ 机器人未启用。');
	if (request.headers.get(credentials.provider === 'official' ? 'x-bot-appid' : 'x-self-id') !== credentials.selfId)
		throw new ApiError(401, 'Wrong bot application.');
	const reader = request.body?.getReader();
	if (!reader) throw new ApiError(400, 'Missing body.');
	const chunks: Uint8Array[] = [];
	let length = 0;
	while (true) {
		const chunk = await reader.read();
		if (chunk.done) break;
		length += chunk.value.length;
		if (length > 65536) {
			await reader.cancel();
			throw new ApiError(413, 'Payload too large.');
		}
		chunks.push(chunk.value);
	}
	const raw = Buffer.concat(chunks);
	if (credentials.provider !== 'official' && !verifyQq(credentials.secret, request.headers, raw))
		throw new ApiError(401, 'Invalid QQ signature.');
	let payload: unknown;
	try {
		payload = JSON.parse(raw.toString('utf8'));
	} catch {
		throw new ApiError(400, 'Invalid JSON.');
	}
	if (credentials.provider === 'official') {
		const p = payload as {op?:number;d?:{plain_token?:unknown;event_ts?:unknown}};
		if (p.op === 13 && typeof p.d?.plain_token === 'string' && p.d.plain_token.length <= 2000 &&
			typeof p.d.event_ts === 'string' && /^\d{10}$/.test(p.d.event_ts) && Math.abs(Date.now()/1000-Number(p.d.event_ts))<=300) {
			return Response.json(officialValidation(credentials.secret,p.d.plain_token,p.d.event_ts));
		}
		if (!verifyOfficial(credentials.secret,request.headers,raw)) throw new ApiError(401,'Invalid official QQ signature.');
		await acceptOfficialEvent(getEnv(),payload,credentials.selfId);
		return Response.json({op:12});
	}
	const message = oneBotMessage(payload, credentials.selfId);
	if (!message) return new Response(null, { status: 204 });
	const policy = qqPolicies().find((p) => p.groups.includes(message.groupId));
	if (!policy) return new Response(null, { status: 204 });
	await getEnv().db.transaction(async (tx) => {
		await tx.execute(
			sql`SELECT pg_advisory_xact_lock(hashtextextended(${`qq-event:${message.memberId}`},0))`
		);
		const duplicate = await tx.execute(sql`SELECT 1 FROM qq_inbox WHERE id=${message.id}`);
		if (duplicate.length) return;
		const [count] = await tx.execute<{ n: string }>(
			sql`SELECT count(*) AS n FROM qq_inbox WHERE member_id=${message.memberId} AND created_at>now()-interval '1 minute'`
		);
		if (Number(count.n) >= 10) return;
		await tx.execute(
			sql`INSERT INTO qq_inbox(id,server_id,group_id,member_id,content) VALUES(${message.id},${policy.serverId},${message.groupId},${message.memberId},${message.content}) ON CONFLICT DO NOTHING`
		);
	});
	return new Response(null, { status: 204 });
});
