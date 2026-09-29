import { createPrivateKey, createPublicKey, sign, verify } from 'node:crypto';

function privateKey(secret: string) {
	if (!secret) throw new Error('QQ secret is required.');
	const source = Buffer.from(secret);
	const seed = Buffer.alloc(32);
	for (let i = 0; i < 32; i++) seed[i] = source[i % source.length];
	return createPrivateKey({
		key: Buffer.concat([Buffer.from('302e020100300506032b657004220420', 'hex'), seed]),
		format: 'der',
		type: 'pkcs8'
	});
}
export function qqSignature(secret: string, timestamp: string, body: string | Uint8Array): string {
	return sign(
		null,
		Buffer.concat([Buffer.from(timestamp), Buffer.from(body)]),
		privateKey(secret)
	).toString('hex');
}
export function verifyQq(
	secret: string,
	headers: Headers,
	body: Uint8Array,
	now = Date.now()
): boolean {
	const timestamp = headers.get('x-signature-timestamp') || '';
	const signature = headers.get('x-signature-ed25519') || '';
	if (
		!/^\d{10}$/.test(timestamp) ||
		!/^[a-f0-9]{128}$/i.test(signature) ||
		Math.abs(now / 1000 - Number(timestamp)) > 300
	)
		return false;
	return verify(
		null,
		Buffer.concat([Buffer.from(timestamp), Buffer.from(body)]),
		createPublicKey(privateKey(secret)),
		Buffer.from(signature, 'hex')
	);
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
	private token = '';
	private until = 0;
	constructor(
		private appId: string,
		private secret: string,
		private fetcher: typeof fetch = fetch
	) {}
	async reply(group: string, messageId: string, content: string) {
		if (Date.now() >= this.until) {
			const response = await this.fetcher('https://bots.qq.com/app/getAppAccessToken', {
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({ appId: this.appId, clientSecret: this.secret }),
				signal: AbortSignal.timeout(10000)
			});
			if (!response.ok) throw new Error(`QQ token HTTP ${response.status}`);
			const data = await response.json();
			if (!data.access_token || !Number.isFinite(Number(data.expires_in)))
				throw new Error('Invalid QQ token response.');
			this.token = data.access_token;
			this.until = Date.now() + Math.max(1, Number(data.expires_in) - 60) * 1000;
		}
		const response = await this.fetcher(
			`https://api.sgroup.qq.com/v2/groups/${encodeURIComponent(group)}/messages`,
			{
				method: 'POST',
				headers: {
					'content-type': 'application/json',
					authorization: `QQBot ${this.token}`,
					'X-Union-Appid': this.appId
				},
				body: JSON.stringify({
					content: content.slice(0, 3500),
					msg_type: 0,
					msg_id: messageId,
					msg_seq: 1
				}),
				signal: AbortSignal.timeout(10000)
			}
		);
		if (!response.ok) {
			if (response.status === 401) this.until = 0;
			throw new Error(`QQ reply HTTP ${response.status}`);
		}
	}
}
