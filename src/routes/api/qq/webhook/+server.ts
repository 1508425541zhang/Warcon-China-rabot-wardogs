import { z } from 'zod';
import { sql } from 'drizzle-orm';
import { getEnv } from '$lib/server/env';
import { ApiError, apiJson, route } from '$lib/server/http';
import { qqCredentials, qqPolicies } from '$lib/server/qq/config';
import { qqSignature, verifyQq } from '$lib/server/qq/protocol';

const messageSchema = z.object({
	id: z.string().min(1).max(256),
	group_openid: z.string().min(1).max(128),
	author: z.object({ member_openid: z.string().min(1).max(128) }),
	content: z.string().max(2000)
});
export const POST = route(async ({ request }) => {
	const credentials = qqCredentials();
	if (!credentials) throw new ApiError(404, 'QQ 机器人未启用。');
	if (request.headers.get('x-bot-appid') !== credentials.appId)
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
	if (!verifyQq(credentials.secret, request.headers, raw))
		throw new ApiError(401, 'Invalid QQ signature.');
	let payload: { op?: number; t?: string; d?: unknown };
	try {
		payload = JSON.parse(raw.toString('utf8'));
	} catch {
		throw new ApiError(400, 'Invalid JSON.');
	}
	if (!payload || typeof payload !== 'object') throw new ApiError(400, 'Invalid event.');
	if (payload.op === 13) {
		const parsed = z
			.object({ plain_token: z.string().min(1).max(1024), event_ts: z.string().regex(/^\d{10}$/) })
			.safeParse(payload.d);
		if (!parsed.success) throw new ApiError(400, 'Invalid challenge.');
		return apiJson({
			plain_token: parsed.data.plain_token,
			signature: qqSignature(credentials.secret, parsed.data.event_ts, parsed.data.plain_token)
		});
	}
	if (payload.op === 1) return apiJson({ op: 11, d: payload.d });
	if (payload.op !== 0 || payload.t !== 'GROUP_AT_MESSAGE_CREATE') return apiJson({ op: 12, d: 0 });
	const parsed = messageSchema.safeParse(payload.d);
	if (!parsed.success) throw new ApiError(400, 'Invalid group event.');
	const message = parsed.data;
	const policy = qqPolicies().find((p) => p.groups.includes(message.group_openid));
	if (!policy) return apiJson({ op: 12, d: 0 });
	await getEnv().db.transaction(async (tx) => {
		await tx.execute(
			sql`SELECT pg_advisory_xact_lock(hashtextextended(${`qq-event:${message.author.member_openid}`},0))`
		);
		const duplicate = await tx.execute(sql`SELECT 1 FROM qq_inbox WHERE id=${message.id}`);
		if (duplicate.length) return;
		const [count] = await tx.execute<{ n: string }>(
			sql`SELECT count(*) AS n FROM qq_inbox WHERE member_id=${message.author.member_openid} AND created_at>now()-interval '1 minute'`
		);
		if (Number(count.n) >= 10) return;
		await tx.execute(
			sql`INSERT INTO qq_inbox(id,server_id,group_id,member_id,content) VALUES(${message.id},${policy.serverId},${message.group_openid},${message.author.member_openid},${message.content}) ON CONFLICT DO NOTHING`
		);
	});
	return apiJson({ op: 12, d: 0 });
});
