import { createPrivateKey, createPublicKey, sign, verify } from 'node:crypto';
import { z } from 'zod';

export const OFFICIAL_API = 'https://api.bot.qq.com';
const openid = z.string().regex(/^[A-Za-z0-9_-]{16,128}$/);
const eventSchema = z.object({
	op: z.literal(0),
	t: z.enum(['GROUP_AT_MESSAGE_CREATE', 'GROUP_MESSAGE_CREATE']),
	d: z.object({
		id: z.string().min(1).max(512), group_openid: openid,
		content: z.string().max(4000), timestamp: z.string(),
		author: z.object({ member_openid: openid.optional(),
			user_openid: openid.optional(), bot: z.boolean().optional() }),
		attachments: z.array(z.unknown()).optional()
	})
});
export function officialMessage(payload: unknown, appId: string, now = Date.now()) {
	const parsed = eventSchema.safeParse(payload);
	if (!parsed.success) return null;
	const d = parsed.data.d;
	const member = d.author.member_openid || d.author.user_openid;
	const time = Date.parse(d.timestamp);
	const content = d.content.replace(/^\s*<@!?\d+>\s*/, '').trim();
	if (!member || d.author.bot || !Number.isFinite(time) || Math.abs(now - time) > 300000 ||
		d.attachments?.length || !/^[/!！]\S/.test(content) || content.length > 2000) return null;
	return { id: `official:${appId}:${d.group_openid}:${Buffer.from(d.id).toString('base64url')}`,
		groupId: d.group_openid, memberId: `official:${appId}:${member}`, content };
}
export function validQqIdentity(member: string) {
	return /^ob11:[1-9]\d+$/.test(member) || /^official:[1-9]\d{4,15}:[A-Za-z0-9_-]{16,128}$/.test(member);
}
function key(secret: string) {
	if (!secret) throw new Error('Missing official QQ secret');
	let seed = Buffer.from(secret);
	while (seed.length < 32) seed = Buffer.concat([seed, seed]);
	return createPrivateKey({ key: Buffer.concat([Buffer.from('302e020100300506032b657004220420', 'hex'), seed.subarray(0,32)]), format: 'der', type: 'pkcs8' });
}
export function officialValidation(secret: string, token: string, timestamp: string) {
	return { plain_token: token, signature: sign(null, Buffer.from(timestamp + token), key(secret)).toString('hex') };
}
export function verifyOfficial(secret: string, headers: Headers, body: Uint8Array, now = Date.now()) {
	const timestamp = headers.get('x-signature-timestamp') || '';
	const signature = headers.get('x-signature-ed25519') || '';
	if (!/^\d{10}$/.test(timestamp) || Math.abs(now/1000 - Number(timestamp)) > 300 || !/^[a-fA-F0-9]{128}$/.test(signature)) return false;
	return verify(null, Buffer.concat([Buffer.from(timestamp), body]), createPublicKey(key(secret)), Buffer.from(signature, 'hex'));
}
/** Calls Tencent's OpenAPI directly. No QQ client, OneBot or third-party gateway. */
export class OfficialQqClient {
	private accessToken = '';
	private expiresAt = 0;
	private pending: Promise<string> | null = null;
	constructor(readonly appId: string, private secret: string, private fetcher: typeof fetch = fetch) {}
	async token(): Promise<string> {
		if (this.accessToken && Date.now() < this.expiresAt) return this.accessToken;
		if (this.pending) return this.pending;
		this.pending = (async () => {
			const r = await this.fetcher(OFFICIAL_API + '/app/getAppAccessToken', { method: 'POST', redirect: 'error',
				headers: { 'content-type': 'application/json' }, body: JSON.stringify({ appId: this.appId, clientSecret: this.secret }), signal: AbortSignal.timeout(10000) });
			const d = await r.json();
			if (!r.ok || (d.code && d.code !== 0) || typeof d.access_token !== 'string' || !Number.isFinite(Number(d.expires_in)) || Number(d.expires_in) <= 0) throw new Error('Official QQ credentials rejected');
			this.accessToken = d.access_token;
			this.expiresAt = Date.now() + Math.max(1, Number(d.expires_in)-60)*1000;
			return this.accessToken;
		})().finally(() => { this.pending = null; });
		return this.pending;
	}
	async request(path: string, body?: unknown) {
		const token = await this.token();
		const r = await this.fetcher(OFFICIAL_API + path, { method: body === undefined ? 'GET' : 'POST', redirect: 'error',
			headers: { authorization: 'QQBot ' + token, 'X-Union-Appid': this.appId, 'content-type': 'application/json' },
			body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(10000) });
		const d = await r.json();
		if (r.status === 401) { this.accessToken = ''; this.expiresAt = 0; }
		if (!r.ok || (d.code && d.code !== 0)) throw new Error(`Official QQ API rejected request (${r.status}, ${Number(d.code)||0})`);
		return d;
	}
	async loggedIn(appId: string) { try { await this.token(); return appId === this.appId; } catch { return false; } }
	async reply(group: string, id: string, content: string) {
		if (!openid.safeParse(group).success) throw new Error('Invalid official QQ group OpenID');
		const prefix = `official:${this.appId}:${group}:`;
		const body: Record<string,unknown> = { content: content.slice(0,3500), msg_type: 0 };
		if (id.startsWith(prefix)) { body.msg_id = Buffer.from(id.slice(prefix.length),'base64url').toString('utf8'); body.msg_seq = 1; }
		const result = await this.request('/v2/groups/' + encodeURIComponent(group) + '/messages', body);
		if (typeof result.id !== 'string' || !result.id) throw new Error('Official QQ did not confirm delivery');
	}
}

/** Official gateway heartbeat, resume and bounded reconnect. One active socket per runtime. */
export class OfficialGateway {
	private socket: WebSocket | null = null;
	private heartbeat: ReturnType<typeof setInterval> | null = null;
	private retry: ReturnType<typeof setTimeout> | null = null;
	private closed = false;
	private sequence: number | null = null;
	private session = '';
	private ack = true;
	private attempts = 0;
	constructor(private client: OfficialQqClient, private receive: (payload: unknown) => Promise<void>) {}
	start() { void this.connect(); }
	stop() { this.closed = true; if (this.retry) clearTimeout(this.retry); this.disconnect(); }
	private disconnect() { if (this.heartbeat) clearInterval(this.heartbeat); this.heartbeat = null; this.socket?.close(); this.socket = null; }
	private reconnect() {
		this.disconnect();
		if (this.closed || this.retry) return;
		const delay = Math.min(60000, 1000 * 2 ** Math.min(this.attempts++,6));
		this.retry = setTimeout(() => { this.retry = null; void this.connect(); }, delay);
	}
	private async connect() {
		try {
			const gateway = await this.client.request('/gateway');
			const url = new URL(gateway.url);
			if (url.protocol !== 'wss:' || !['api.bot.qq.com','api.sgroup.qq.com'].includes(url.hostname)) throw new Error('Invalid official gateway');
			const token = await this.client.token();
			if (this.closed) return;
			const socket = new WebSocket(url.toString()); this.socket = socket;
			socket.onclose = () => { if (this.socket === socket) this.reconnect(); };
			socket.onerror = () => { if (this.socket === socket) this.reconnect(); };
			socket.onmessage = async (event) => {
				if (this.closed || this.socket !== socket) return;
				try {
					const p = JSON.parse(String(event.data));
					if (typeof p.s === 'number') this.sequence = p.s;
					if (p.op === 10) {
						const ms = Number(p.d?.heartbeat_interval);
						if (!Number.isFinite(ms) || ms < 1000 || ms > 120000) throw new Error('Invalid heartbeat');
						this.ack = true;
						if (this.heartbeat) clearInterval(this.heartbeat);
						this.heartbeat = setInterval(() => {
							if (!this.ack) { this.reconnect(); return; }
							this.ack = false; socket.send(JSON.stringify({ op:1, d:this.sequence }));
						}, ms);
						socket.send(JSON.stringify(this.session ? {op:6,d:{token:'QQBot '+token,session_id:this.session,seq:this.sequence}} :
							{op:2,d:{token:'QQBot '+token,intents:1<<25,shard:[0,1],properties:{$os:'linux',$browser:'warcon',$device:'warcon'}}}));
					} else if (p.op === 11) this.ack = true;
					else if (p.op === 7 || p.op === 9) { if (p.op === 9 && !p.d) { this.session = ''; this.sequence = null; } this.reconnect(); }
					else if (p.op === 0) {
						if (p.t === 'READY') { this.session = p.d.session_id; this.attempts = 0; console.info('[qq-official] gateway ready'); }
						await this.receive(p);
					}
				} catch { console.error('[qq-official] gateway event failed'); this.reconnect(); }
			};
		} catch { console.error('[qq-official] gateway connection failed'); this.reconnect(); }
	}
}
