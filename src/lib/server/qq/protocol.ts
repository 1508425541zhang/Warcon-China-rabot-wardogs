import { createHmac, timingSafeEqual } from 'node:crypto';
import { z } from 'zod';

export function qqSignature(secret: string, body: string | Uint8Array): string {
	return 'sha1=' + createHmac('sha1', secret).update(body).digest('hex');
}
export function verifyQq(secret: string, headers: Headers, body: Uint8Array): boolean {
	const signature = headers.get('x-signature') || '';
	return (
		/^sha1=[a-f0-9]{40}$/.test(signature) &&
		timingSafeEqual(Buffer.from(signature), Buffer.from(qqSignature(secret, body)))
	);
}
const id = z
	.union([z.number().int().safe().positive(), z.string().regex(/^[1-9]\d{0,15}$/)])
	.transform(String);
const eventSchema = z.object({
	time: z.number().int(),
	self_id: id,
	post_type: z.literal('message'),
	message_type: z.literal('group'),
	sub_type: z.literal('normal'),
	group_id: id,
	user_id: id,
	message_id: z
		.union([z.number().int().safe(), z.string().regex(/^-?\d{1,20}$/)])
		.transform(String),
	anonymous: z.unknown().optional(),
	message: z.union([
		z.string().max(4000),
		z.array(z.object({ type: z.string(), data: z.record(z.string(), z.unknown()) })).max(100)
	])
});
/** Ignore non-commands, self messages and anonymous senders. Never interpret CQ in replies. */
export function oneBotMessage(payload: unknown, selfId: string, now = Date.now()) {
	const parsed = eventSchema.safeParse(payload);
	if (!parsed.success) return null;
	const e = parsed.data;
	if (
		e.self_id !== selfId ||
		e.user_id === selfId ||
		e.anonymous ||
		Math.abs(now / 1000 - e.time) > 300
	)
		return null;
	let text = '';
	if (typeof e.message === 'string') {
		text = e.message.replace(new RegExp('^\\s*\\[CQ:at,qq=' + selfId + '\\]\\s*'), '');
		if (/\[CQ:/.test(text)) return null;
		text = text
			.replace(/&#91;/g, '[')
			.replace(/&#93;/g, ']')
			.replace(/&#44;/g, ',')
			.replace(/&amp;/g, '&');
	} else {
		let mentioned = false;
		for (const segment of e.message) {
			if (
				segment.type === 'at' &&
				String(segment.data.qq) === selfId &&
				!text.trim() &&
				!mentioned
			) {
				mentioned = true;
				continue;
			}
			if (segment.type !== 'text' || typeof segment.data.text !== 'string') return null;
			text += segment.data.text;
		}
	}
	text = text.trim();
	if (!/^[/!！]\S/.test(text) || text.length > 2000) return null;
	return {
		id: `ob11:${selfId}:${e.group_id}:${e.message_id}`,
		groupId: e.group_id,
		memberId: `ob11:${e.user_id}`,
		content: text
	};
}
export function parseCommand(content: string): { name: string; arg: string } {
	const text = content
		.replace(/^\s*<@!?\d+>\s*/, '')
		.trim()
		.replace(/^[/!！]/, '');
	const match = /^(\S+)(?:\s+([\s\S]*))?$/.exec(text);
	return { name: match?.[1] || '帮助', arg: match?.[2]?.trim() || '' };
}
export interface RosterPlayer {
	steamId: string;
	name: string;
	faction: string | null;
}
export function friendlyTargets(roster: RosterPlayer[], steamId: string): RosterPlayer[] {
	const own = roster.find((p) => p.steamId === steamId);
	if (!own?.faction) throw new Error('你须在本服在线且已加入阵营。');
	return [
		...new Map(
			roster
				.filter((p) => p.faction === own.faction && /^\d{17}$/.test(p.steamId))
				.map((p) => [p.steamId, p])
		).values()
	];
}
export function warmAward(oldMs: number, gapMs: number, rate: number) {
	const totalMs = oldMs + gapMs;
	return { totalMs, points: (Math.floor(totalMs / 60000) - Math.floor(oldMs / 60000)) * rate };
}

export class QqClient {
	constructor(
		private url: string,
		private token: string,
		private fetcher: typeof fetch = fetch
	) {}
	async loggedIn(selfId: string) {
		try {
			const response = await this.fetcher(`${this.url}/get_login_info`, {
				method: 'POST',
				redirect: 'error',
				headers: { 'content-type': 'application/json', authorization: `Bearer ${this.token}` },
				body: '{}',
				signal: AbortSignal.timeout(10000)
			});
			if (!response.ok) return false;
			const data = await response.json();
			return data.status === 'ok' && data.retcode === 0 && String(data.data?.user_id) === selfId;
		} catch {
			return false;
		}
	}
	async reply(group: string, _messageId: string, content: string) {
		const response = await this.fetcher(`${this.url}/send_group_msg`, {
			method: 'POST',
			redirect: 'error',
			headers: { 'content-type': 'application/json', authorization: `Bearer ${this.token}` },
			body: JSON.stringify({
				group_id: group,
				message: [{ type: 'text', data: { text: content.slice(0, 3500) } }]
			}),
			signal: AbortSignal.timeout(10000)
		});
		if (!response.ok) throw new Error(`OneBot HTTP ${response.status}`);
		const data = await response.json();
		if (data.status !== 'ok' || data.retcode !== 0 || data.data?.message_id === undefined)
			throw new Error('OneBot did not confirm delivery.');
	}
}
