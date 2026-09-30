import { describe, expect, test } from 'bun:test';
import {
	friendlyTargets,
	parseCommand,
	oneBotMessage,
	qqSignature,
	verifyQq,
	warmAward,
	QqClient
} from './protocol';
import { parsePolicies } from './config';

describe('NapCat OneBot 11 protocol and routing', () => {
	test('HMAC checks raw bytes and rejects tampering and missing signatures', () => {
		const body = Buffer.from('{"中文":"战绩"}');
		const headers = new Headers({ 'x-signature': qqSignature('secret', body) });
		expect(verifyQq('secret', headers, body)).toBe(true);
		expect(verifyQq('secret', headers, Buffer.from('{}'))).toBe(false);
		expect(verifyQq('wrong', headers, body)).toBe(false);
		expect(verifyQq('secret', new Headers(), body)).toBe(false);
	});
	test('normalizes array and CQ messages; rejects stale, anonymous, self and ordinary chat', () => {
		const event = {
			time: Math.floor(Date.now() / 1000),
			self_id: 12345,
			post_type: 'message',
			message_type: 'group',
			sub_type: 'normal',
			group_id: 23456,
			user_id: 34567,
			message_id: -123,
			message: [
				{ type: 'at', data: { qq: '12345' } },
				{ type: 'text', data: { text: ' /战绩 玩家' } }
			]
		};
		expect(oneBotMessage(event, '12345')).toEqual({
			id: 'ob11:12345:23456:-123',
			groupId: '23456',
			memberId: 'ob11:34567',
			content: '/战绩 玩家'
		});
		expect(oneBotMessage({ ...event, message: '[CQ:at,qq=12345] /积分' }, '12345')?.content).toBe(
			'/积分'
		);
		for (const change of [
			{ time: 1 },
			{ self_id: 98765 },
			{ user_id: 12345 },
			{ anonymous: {} },
			{ message: '你好' },
			{ message_type: 'private' },
			{ message: [{ type: 'image', data: { file: 'x' } }] },
			{ message: '[CQ:at,qq=55555] /积分' }
		])
			expect(oneBotMessage({ ...event, ...change }, '12345')).toBeNull();
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
		const p = { serverId: 's', groups: ['12345'], maps: ['a', 'b'] };
		expect(() => parsePolicies(JSON.stringify([p, { ...p, serverId: 't' }]))).toThrow();
		expect(() => parsePolicies(JSON.stringify([{ ...p, voteCost: -1 }]))).toThrow();
	});
	test('sends text segments using Bearer and checks OneBot retcode, not only HTTP', async () => {
		const calls: { url: string; init?: RequestInit }[] = [];
		const fetcher = (async (url: unknown, init?: RequestInit) => {
			calls.push({ url: String(url), init });
			return Response.json({ status: 'ok', retcode: 0, data: { message_id: 42 } });
		}) as typeof fetch;
		await new QqClient('http://127.0.0.1:3001', 'token', fetcher).reply(
			'12345',
			'request',
			'[CQ:at,qq=all]'
		);
		expect(calls[0].url).toBe('http://127.0.0.1:3001/send_group_msg');
		expect(new Headers(calls[0].init?.headers).get('authorization')).toBe('Bearer token');
		expect(JSON.parse(String(calls[0].init?.body))).toEqual({
			group_id: '12345',
			message: [{ type: 'text', data: { text: '[CQ:at,qq=all]' } }]
		});
		for (const data of [
			{ status: 'failed', retcode: 100 },
			{ status: 'async', retcode: 1 },
			{ status: 'ok', retcode: 0, data: {} }
		]) {
			const bad = (async () => Response.json(data)) as unknown as typeof fetch;
			await expect(
				new QqClient('http://127.0.0.1:3001', 'token', bad).reply('12345', 'id', 'text')
			).rejects.toThrow();
		}
	});
});
