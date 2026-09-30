import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import { sql } from 'drizzle-orm';
import { testEnv, hasTestDb } from './db';
import type { Env } from '$lib/server/env';
import { creditWarmth, debit, wallet, reconcileOrder } from '$lib/server/qq/economy';
import { bindAccount, createLinkCode, linkedAccount } from '$lib/server/qq/identity';
import { castVote, closeVotes, createPurchase, playerSummary } from '$lib/server/qq/community';
import { parsePolicies } from '$lib/server/qq/config';
import { setGateway, type Gateway } from '$lib/server/gateway';
import { localGateway } from '$lib/server/gateway-local';
import { toSessionUser, getServer } from '$lib/server/access';
import { POST as webhook } from '../routes/api/qq/webhook/+server';
import { qqSignature, QqClient } from '$lib/server/qq/protocol';
import { deliverOrder, processMessage } from '$lib/server/qq/runtime';
import {
	GET as communityGet,
	POST as communityPost
} from '../routes/api/servers/[id]/community/+server';
import { callApi } from './call';
import { seedWorld } from './world';

describe.skipIf(!hasTestDb)('QQ community database invariants', () => {
	let env: Env;
	const steam = '76561198000000001';
	const other = '76561198000000002';
	const policy = parsePolicies(
		JSON.stringify([
			{ serverId: 'qq-test', groups: ['23456'], maps: ['mapA', 'mapB'], pointsPerMinute: 10 }
		])
	)[0];
	const saved = {
		policy: process.env.QQ_BOT_POLICIES,
		url: process.env.ONEBOT_HTTP_URL,
		token: process.env.ONEBOT_ACCESS_TOKEN,
		self: process.env.ONEBOT_SELF_ID,
		secret: process.env.ONEBOT_EVENT_SECRET
	};
	beforeAll(async () => {
		env = await testEnv();
		process.env.QQ_BOT_POLICIES = JSON.stringify([policy]);
		process.env.ONEBOT_HTTP_URL = 'http://127.0.0.1:3001';
		process.env.ONEBOT_ACCESS_TOKEN = 'test';
		process.env.ONEBOT_SELF_ID = '12345';
		process.env.ONEBOT_EVENT_SECRET = 'secret';
		await env.db.execute(
			sql`INSERT INTO organizations(id,name,slug) VALUES('qq-org','QQ test','qq-test')`
		);
		await env.db.execute(
			sql`INSERT INTO servers(id,org_id,name,host,port,password_enc) VALUES('qq-test','qq-org','QQ server','demo',1,'test')`
		);
		setGateway({
			...localGateway,
			run: async (_env, _server, action) =>
				action === 'players'
					? {
							players: [
								{ steamId: steam, name: 'A', faction: 'red' },
								{ steamId: other, name: 'B', faction: 'red' },
								{ steamId: '76561198000000003', name: 'enemy', faction: 'blue' }
							]
						}
					: {}
		} as Gateway);
	}, 120000);
	afterAll(() => {
		setGateway(localGateway);
		for (const [key, value] of Object.entries({
			QQ_BOT_POLICIES: saved.policy,
			ONEBOT_HTTP_URL: saved.url,
			ONEBOT_ACCESS_TOKEN: saved.token,
			ONEBOT_SELF_ID: saved.self,
			ONEBOT_EVENT_SECRET: saved.secret
		})) {
			if (value === undefined) delete process.env[key];
			else process.env[key] = value;
		}
	});
	test('automatic warmth: carry, replay, high population, untrusted gaps and rollback', async () => {
		const tick = async (at: number, gap: number, count = 2, trusted = true) =>
			env.db.transaction((tx) =>
				creditWarmth(tx, 'qq-test', new Date(at), [steam, other], gap, count, trusted)
			);
		await tick(100000, 30000);
		await tick(130000, 30000);
		await tick(130000, 30000);
		expect((await wallet(env, 'qq-test', steam)).balance).toBe(10);
		await tick(160000, 30000, 100);
		await tick(190000, 30000, 2, false);
		expect((await wallet(env, 'qq-test', steam)).warmMinutes).toBe(1);
		await expect(
			env.db.transaction(async (tx) => {
				await creditWarmth(tx, 'qq-test', new Date(200000), [steam], 60000, 1, true);
				throw new Error('rollback');
			})
		).rejects.toThrow();
		expect((await wallet(env, 'qq-test', steam)).balance).toBe(10);
	});
	test('concurrent debits cannot overdraw and losing debit has no ledger row', async () => {
		const results = await Promise.allSettled(
			['a', 'b'].map((id) =>
				env.db.transaction((tx) => debit(tx, 'qq-test', other, 10, `test-${id}`, 'test'))
			)
		);
		expect(results.filter((r) => r.status === 'fulfilled')).toHaveLength(1);
		expect((await wallet(env, 'qq-test', other)).balance).toBe(0);
	});
	test('QQ binding consumes a code only after verified Steam login', async () => {
		await env.db.execute(
			sql`INSERT INTO "user"(id,name,email,created_at,updated_at) VALUES('qq-user','QQ user','qq-test@example.test',now(),now())`
		);
		const actor = toSessionUser({ id: 'qq-user', name: 'QQ user' });
		const code = await createLinkCode(env, 'qq-test', 'member');
		await expect(bindAccount(env, code, actor)).rejects.toThrow();
		await env.db.execute(
			sql`INSERT INTO account(id,account_id,provider_id,user_id,created_at,updated_at) VALUES('qq-account',${steam},'steam','qq-user',now(),now())`
		);
		await bindAccount(env, code, actor);
		expect((await linkedAccount(env, 'qq-test', 'member')).steamId).toBe(steam);
		await expect(bindAccount(env, code, actor)).rejects.toThrow();
		await env.db.execute(sql`DELETE FROM account WHERE id='qq-account'`);
		await expect(linkedAccount(env, 'qq-test', 'member')).rejects.toThrow();
	});
	test('a duplicate vote charges once and close creates one map order', async () => {
		await env.db.execute(
			sql`INSERT INTO qq_votes(id,server_id,maps,ends_at) VALUES('test-vote','qq-test','["mapA","mapB"]',now()+interval '1 minute')`
		);
		await Promise.all([castVote(env, policy, steam, '1'), castVote(env, policy, steam, '1')]);
		expect((await wallet(env, 'qq-test', steam)).balance).toBe(0);
		await env.db.execute(
			sql`UPDATE qq_votes SET ends_at=now()-interval '1 second' WHERE id='test-vote'`
		);
		await Promise.all([closeVotes(env), closeVotes(env)]);
		const rows = await env.db.execute(sql`SELECT * FROM qq_orders WHERE id='map:test-vote'`);
		expect(rows).toHaveLength(1);
		await env.db.execute(sql`UPDATE qq_orders SET state='unknown' WHERE id='map:test-vote'`);
		await reconcileOrder(env, 'qq-test', 'map:test-vote', true, 'operator');
		expect((await wallet(env, 'qq-test', steam)).balance).toBe(10);
	});
	test('friendly purchase snapshots only friends, retries do not debit, refunds are once only', async () => {
		await env.db.transaction((tx) =>
			creditWarmth(tx, 'qq-test', new Date(300000), [steam], 60000, 1, true)
		);
		await createPurchase(env, policy, steam, 'test-broadcast', 'friendly', '守住阵地');
		await createPurchase(env, policy, steam, 'test-broadcast', 'friendly', '守住阵地');
		expect((await wallet(env, 'qq-test', steam)).balance).toBe(0);
		const targets = await env.db.execute(
			sql`SELECT steam_id FROM qq_deliveries WHERE order_id='test-broadcast'`
		);
		expect(targets).toHaveLength(2);
		await expect(
			createPurchase(env, policy, steam, 'test-broadcast', 'friendly', '不同内容')
		).rejects.toThrow();
		await env.db.execute(sql`UPDATE qq_orders SET state='partial' WHERE id='test-broadcast'`);
		await reconcileOrder(env, 'qq-test', 'test-broadcast', true, 'operator');
		await expect(
			reconcileOrder(env, 'qq-test', 'test-broadcast', true, 'operator')
		).rejects.toThrow();
		expect((await wallet(env, 'qq-test', steam)).balance).toBe(20);
	});
	test('webhook authenticates and persists one event before acknowledging', async () => {
		const raw = JSON.stringify({
			time: Math.floor(Date.now() / 1000),
			self_id: 12345,
			post_type: 'message',
			message_type: 'group',
			sub_type: 'normal',
			message_id: 123,
			group_id: 23456,
			user_id: 34567,
			message: '/帮助'
		});
		const make = (signature: string) =>
			({
				request: new Request('http://localhost/api/qq/webhook', {
					method: 'POST',
					headers: {
						'x-self-id': '12345',
						'x-signature': signature
					},
					body: raw
				})
			}) as Parameters<typeof webhook>[0];
		expect((await webhook(make('bad'))).status).toBe(401);
		const signature = qqSignature('secret', raw);
		expect((await webhook(make(signature))).status).toBe(204);
		await webhook(make(signature));
		expect(
			await env.db.execute(sql`SELECT id FROM qq_inbox WHERE id='ob11:12345:23456:123'`)
		).toHaveLength(1);
		let sent = false;
		await processMessage(
			env,
			new QqClient('http://127.0.0.1:3001', 'test', (async (_url: unknown, init?: RequestInit) => {
				const body = JSON.parse(String(init?.body));
				expect(body.group_id).toBe('23456');
				expect(body.message[0].data.text).toContain('/服务器');
				sent = true;
				return Response.json({ status: 'ok', retcode: 0, data: { message_id: 900 } });
			}) as typeof fetch)
		);
		expect(sent).toBe(true);
		const [done] = await env.db.execute(
			sql`SELECT reply_state FROM qq_inbox WHERE id='ob11:12345:23456:123'`
		);
		expect(done.reply_state).toBe('done');
	});
	test('broadcast delivery rechecks changing factions and never repeats an uncertain send', async () => {
		let reads = 0;
		const sent: string[] = [];
		setGateway({
			...localGateway,
			run: async (_env, _server, action, params) => {
				if (action === 'players') {
					reads++;
					return {
						players: [
							{ steamId: steam, faction: 'red' },
							{ steamId: other, faction: reads === 1 ? 'red' : 'blue' }
						]
					};
				}
				if (action === 'whisper') {
					sent.push(String(params.steamId));
					throw new Error('timeout after send');
				}
				throw new Error('unexpected action');
			}
		} as Gateway);
		const params = { message: 'test', faction: 'red', hours: 0, map: '' };
		await env.db.execute(
			sql`INSERT INTO qq_orders(id,server_id,steam_id,kind,params,cost) VALUES('delivery-test','qq-test',${steam},'friendly',(${JSON.stringify(params)}::text)::jsonb,20)`
		);
		for (const id of [steam, other])
			await env.db.execute(
				sql`INSERT INTO qq_deliveries(order_id,steam_id) VALUES('delivery-test',${id})`
			);
		const order = {
			id: 'delivery-test',
			server_id: 'qq-test',
			steam_id: steam,
			kind: 'friendly',
			params,
			created_at: new Date()
		};
		await deliverOrder(env, order);
		await deliverOrder(env, order);
		expect(sent).toEqual([steam]);
		const rows = await env.db.execute(
			sql`SELECT steam_id,state FROM qq_deliveries WHERE order_id='delivery-test' ORDER BY steam_id`
		);
		expect(rows.map((r) => r.state)).toEqual(['unknown', 'skipped']);
	});
	test('community API denies unauthenticated, cross-tenant and read-only spending', async () => {
		const world = await seedWorld(env);
		for (const principal of ['anon', 'outsider', 'keyElsewhere', 'keyView'] as const) {
			const result = await callApi(communityPost, world.users[principal], {
				method: 'POST',
				params: { id: world.server.id },
				body: { action: 'reserve', steamId: steam, requestId: 'request-1234567890' }
			});
			expect([401, 403, 404]).toContain(result.status);
		}
		const result = await callApi(communityGet, world.users.outsider, {
			params: { id: world.server.id },
			query: `view=economy&steamId=${steam}`
		});
		expect(result.status).toBe(404);
	});
	test('query API enforces scope, paginates and excludes unlisted upstream fields', async () => {
		const world = await seedWorld(env);
		setGateway({
			...localGateway,
			run: async (_env, _server, action) =>
				action === 'players'
					? {
							players: Array.from({ length: 23 }, (_, i) => ({
								steamId: String(i),
								name: `Player ${i}`,
								faction: 'red',
								ip: 'private-ip'
							}))
						}
					: {
							serverName: 'Test',
							map: 'MapA',
							playerCount: 23,
							maxPlayers: 100,
							password: 'secret'
						}
		} as Gateway);
		for (const view of ['status', 'players', 'maps']) {
			const denied = await callApi(communityGet, world.users.outsider, {
				params: { id: world.server.id },
				query: `view=${view}`
			});
			expect(denied.status).toBe(404);
		}
		const result = await callApi(communityGet, world.users.keyView, {
			params: { id: world.server.id },
			query: 'view=players&page=2'
		});
		expect(result.status).toBe(200);
		const body = result.body as { players: unknown[] };
		expect(body.players).toHaveLength(3);
		expect(body.players[0]).toEqual({ steamId: '20', name: 'Player 20', faction: 'red' });
		const status = await callApi(communityGet, world.users.keyView, {
			params: { id: world.server.id },
			query: 'view=status'
		});
		expect(status.status).toBe(200);
		expect(status.body).not.toHaveProperty('password');
		for (const query of ['view=players&page=0', 'view=unknown'])
			expect(
				(
					await callApi(communityGet, world.users.keyView, {
						params: { id: world.server.id },
						query
					})
				).status
			).toBe(400);
	});
	test('expired queued commands do not spend or call QQ', async () => {
		await env.db.execute(sql`UPDATE qq_inbox SET state='done'`);
		await env.db.execute(
			sql`INSERT INTO qq_inbox(id,server_id,group_id,member_id,content,created_at) VALUES('old-message','qq-test','23456','member','/优先队列',now()-interval '10 minutes')`
		);
		let fetched = false;
		const client = new QqClient('app', 'secret', (async () => {
			fetched = true;
			throw new Error('must not send');
		}) as unknown as typeof fetch);
		await processMessage(env, client);
		expect(fetched).toBe(false);
		const [row] = await env.db.execute(
			sql`SELECT reply_state FROM qq_inbox WHERE id='old-message'`
		);
		expect(row.reply_state).toBe('expired');
		expect(
			await env.db.execute(sql`SELECT 1 FROM qq_orders WHERE id='qq:old-message'`)
		).toHaveLength(0);
	});
	test('battle timeline stays within the selected server', async () => {
		await env.db.execute(
			sql`INSERT INTO servers(id,org_id,name,host,port,password_enc) VALUES('qq-other','qq-org','Other','demo',1,'test')`
		);
		await env.db.execute(
			sql`INSERT INTO player_sessions(server_id,steam_id,name,joined_at,last_seen) VALUES('qq-test',${steam},'Player',now(),now())`
		);
		for (const serverId of ['qq-test', 'qq-other'])
			await env.db.execute(
				sql`INSERT INTO kills(ts,server_id,event_id,instance_id,match_id,event_time,map,killer_steam_id,killer_name,victim_steam_id,victim_name,tags) VALUES(now(),${serverId},${serverId},'instance','match',1,'mapA',${steam},'Player',${other},'Opponent','[]')`
			);
		const server = await getServer(env, 'qq-test');
		const result = await playerSummary(env, server!, steam);
		expect(result.timeline).toHaveLength(1);
		expect(result.timeline[0].action).toBe('击杀');
		expect(result.summary).toContain('Opponent');
	});
	test('an old vote stays expired after restart rather than becoming a fresh map command', async () => {
		await env.db.execute(
			sql`INSERT INTO qq_votes(id,server_id,maps,ends_at) VALUES('old-vote','qq-test','["mapA","mapB"]',now()-interval '1 hour')`
		);
		await env.db.execute(
			sql`INSERT INTO qq_ballots(vote_id,steam_id,map) VALUES('old-vote',${steam},'mapA')`
		);
		await closeVotes(env);
		const [order] = await env.db.execute(sql`SELECT * FROM qq_orders WHERE id='map:old-vote'`);
		await expect(deliverOrder(env, order as Parameters<typeof deliverOrder>[1])).rejects.toThrow(
			'投票结果已过期'
		);
	});
});
