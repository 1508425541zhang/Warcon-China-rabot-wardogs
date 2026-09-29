import { describe, expect, test } from 'bun:test';
import {
	friendlyTargets,
	parseCommand,
	qqSignature,
	verifyQq,
	warmAward,
	QqClient
} from './protocol';
import { parsePolicies } from './config';

describe('QQ official protocol and routing', () => {
	test('signed raw body, stale timestamps and tampering', () => {
		const now = Date.now();
		const ts = String(Math.floor(now / 1000));
		const body = Buffer.from('{"op":0,"中文":"战绩"}');
		const headers = new Headers({
			'x-signature-timestamp': ts,
			'x-signature-ed25519': qqSignature('test-secret', ts, body)
		});
		expect(verifyQq('test-secret', headers, body, now)).toBe(true);
		expect(verifyQq('test-secret', headers, Buffer.from('{}'), now)).toBe(false);
		expect(verifyQq('wrong-secret', headers, body, now)).toBe(false);
		expect(verifyQq('test-secret', headers, body, now + 301000)).toBe(false);
	});
	test('command retains message text; recipients exclude enemies and unknown factions', () => {
		expect(parseCommand('<@123> /友方广播 守住 A 点')).toEqual({
			name: '友方广播',
			arg: '守住 A 点'
		});
		const p = (id: string, faction: string | null) => ({
			steamId: `7656119800000000${id}`,
			name: id,
			faction
		});
		expect(
			friendlyTargets(
				[p('1', 'A'), p('2', 'A'), p('3', 'B'), p('4', null), p('2', 'A')],
				p('1', 'A').steamId
			).map((p) => p.name)
		).toEqual(['1', '2']);
		expect(() => friendlyTargets([p('1', null)], p('1', null).steamId)).toThrow();
	});
	test('fractional minutes carry forward', () => {
		expect(warmAward(59000, 1500, 2)).toEqual({ totalMs: 60500, points: 2 });
		expect(warmAward(60500, 1000, 2).points).toBe(0);
	});
	test('rejects group overlap and invalid economics', () => {
		const p = { serverId: 's', groups: ['g'], maps: ['a', 'b'] };
		expect(() => parsePolicies(JSON.stringify([p, { ...p, serverId: 't' }]))).toThrow();
		expect(() => parsePolicies(JSON.stringify([{ ...p, voteCost: -1 }]))).toThrow();
	});
	test('official token authorization and passive reply message sequence', async () => {
		const calls: { url: string; init?: RequestInit }[] = [];
		const fetcher = (async (url: unknown, init?: RequestInit) => {
			calls.push({ url: String(url), init });
			return Response.json(
				calls.length === 1 ? { access_token: 'token', expires_in: 7200 } : { id: 'reply' }
			);
		}) as typeof fetch;
		const client = new QqClient('app', 'secret', fetcher);
		await client.reply('group', 'message', '战绩');
		await client.reply('group', 'message2', '积分');
		expect(calls).toHaveLength(3);
		expect(new Headers(calls[1].init?.headers).get('authorization')).toBe('QQBot token');
		expect(JSON.parse(String(calls[1].init?.body))).toEqual({
			content: '战绩',
			msg_type: 0,
			msg_id: 'message',
			msg_seq: 1
		});
	});
});
