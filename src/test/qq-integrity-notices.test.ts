import { afterAll, beforeAll, expect, test } from 'bun:test';
import { sql, eq } from 'drizzle-orm';
import { testEnv, hasTestDb } from './db';
import type { Env } from '$lib/server/env';
import {
	organizations,
	servers,
	outbox,
	integrityCases,
	integrityActions,
	matches
} from '$lib/server/db/schema';
import { applyQqConfiguration, parsePolicies } from '$lib/server/qq/config';
import { enqueueIntegrityNotice, processIntegrityNotice } from '$lib/server/qq/integrity-notices';
import { MODEL_CALIBRATION, CALIBRATION_SHA } from '$lib/server/integrity/model-http';
import type { QqClient } from '$lib/server/qq/protocol';
import { describe } from 'bun:test';

describe.skipIf(!hasTestDb)('Confirmed anti-cheat QQ notices', () => {
	let env: Env;
	const steam = '76561198000000001';
	const policy = parsePolicies(
		JSON.stringify([{ serverId: 'notice-server', groups: ['23456', '34567'], maps: ['A', 'B'] }])
	)[0];
	const configure = (enabled = true) =>
		applyQqConfiguration({
			provider: 'llbot',
			enabled: true,
			url: 'http://127.0.0.1:3001',
			token: 'test',
			secret: 'test',
			selfId: '12345',
			policies: [{ ...policy, antiCheatNotices: enabled }]
		});
	const queued = () =>
		env.db.execute<{ content: string; state: string }>(
			sql`SELECT * FROM qq_integrity_notifications WHERE server_id='notice-server' ORDER BY outbox_id,group_id`
		);
	const create = async (kind = 'integrity', state = 'delivered', source = 'RULE') => {
		const id = crypto.randomUUID();
		await env.db.insert(integrityCases).values({
			id,
			orgId: 'notice-org',
			serverId: 'notice-server',
			steamId: steam,
			createdAt: new Date(),
			confidence: 'B',
			trigger: 'test',
			ruleVersion: 1,
			riskScore: 85,
			riskBreakdown: [],
			snapshot: {}
		});
		await env.db.insert(integrityActions).values({
			id,
			caseId: id,
			orgId: 'notice-org',
			serverId: 'notice-server',
			steamId: steam,
			action: 'KICK',
			source,
			deliveryState: 'delivered'
		});
		return (
			await env.db
				.insert(outbox)
				.values({
					serverId: 'notice-server',
					triggerKind: kind,
					action: 'kick',
					steamId: steam,
					params: { reason: '异常行为：[CQ:at,qq=all]' },
					dedupeKey: id,
					state,
					detail: {
						actionId: id,
						caseId: id,
						reviewerName: '管理员A',
						reviewReason: '录像人工确认',
						playerName: '测试玩家'
					}
				})
				.returning()
		)[0];
	};
	beforeAll(async () => {
		env = await testEnv();
		await env.db.execute(
			sql`UPDATE qq_integrity_notifications SET state='delivered' WHERE state='pending'`
		);
		configure();
		await env.db
			.insert(organizations)
			.values({ id: 'notice-org', name: 'Notice', slug: 'notice-org' });
		await env.db.insert(servers).values({
			id: 'notice-server',
			orgId: 'notice-org',
			name: '通知服',
			host: 'demo',
			port: 1,
			passwordEnc: 'test'
		});
	});
	afterAll(() => applyQqConfiguration(null));
	test('ordinary kicks and unsuccessful delivery cannot announce a kick', async () => {
		for (const kind of ['manual', 'risk_kick', ''])
			await enqueueIntegrityNotice(env.db, await create(kind));
		for (const state of ['pending', 'sending', 'failed', 'unknown', 'skipped'])
			await enqueueIntegrityNotice(env.db, await create('integrity', state));
		expect((await queued()).length).toBe(0);
	});
	test('confirmed rule and administrator kicks preserve reasons and deduplicate per group', async () => {
		const rule = await create();
		await enqueueIntegrityNotice(env.db, rule);
		await enqueueIntegrityNotice(env.db, rule);
		expect((await queued()).length).toBe(2);
		expect((await queued())[0].content).toContain('反作弊规则自动处置');
		expect((await queued())[0].content).toContain('异常行为');
		const review = await create('integrity', 'delivered', 'REVIEW');
		await enqueueIntegrityNotice(env.db, review);
		const text = (await queued())[2].content;
		expect(text).toContain('管理员人工审核 · 管理员A');
		expect(text).toContain('录像人工确认');
		expect(text).toContain('不是模型百分位');
	});
	test('model kicks include actual P97/P99 policy and P95 reference', async () => {
		const [match] = await env.db
			.insert(matches)
			.values({ serverId: 'notice-server', map: 'A', startedAt: new Date() })
			.returning();
		for (const action of ['KICK', 'QUARANTINE_24H']) {
			const row = await create('model_integrity');
			const run = crypto.randomUUID();
			await env.db.execute(
				sql`INSERT INTO integrity_model_runs(id,org_id,server_id,steam_id,match_id,slot,config_revision,state,score,threshold,result,action,action_state) VALUES(${run},'notice-org','notice-server',${steam},${match.id},${row.id},'test','READY',0.1,${MODEL_CALIBRATION.p97},${JSON.stringify({ calibrationSha256: CALIBRATION_SHA })}::jsonb,${action},'delivered')`
			);
			row.detail = { runId: run, modelCalibration: MODEL_CALIBRATION, playerName: '模型玩家' };
			await env.db.update(outbox).set({ detail: row.detail }).where(eq(outbox.id, row.id));
			await enqueueIntegrityNotice(env.db, row);
			const text = (await queued()).at(-1)!.content;
			expect(text).toContain('AI 自动决策');
			expect(text).toContain('参考 P95');
			expect(text).toContain(action === 'KICK' ? 'P97 · 自动踢出' : 'P99 · 隔离24小时');
		}
	});
	test('disabled notices never enqueue', async () => {
		const count = (await queued()).length;
		configure(false);
		await enqueueIntegrityNotice(env.db, await create());
		expect((await queued()).length).toBe(count);
		configure();
	});
	test('offline notices wait; acknowledged sends happen once; uncertain sends never replay', async () => {
		let online = false,
			sends = 0;
		const client = {
			loggedIn: async () => online,
			reply: async () => {
				sends++;
			}
		} as unknown as QqClient;
		await processIntegrityNotice(env, client, '12345');
		expect(sends).toBe(0);
		online = true;
		await processIntegrityNotice(env, client, '12345');
		expect(sends).toBe(1);
		expect((await queued())[0].state).toBe('delivered');
		const uncertain = {
			loggedIn: async () => true,
			reply: async () => {
				sends++;
				throw Error('timeout');
			}
		} as unknown as QqClient;
		await processIntegrityNotice(env, uncertain, '12345');
		expect((await queued())[1].state).toBe('unknown');
		await processIntegrityNotice(env, client, '12345');
		expect((await queued())[1].state).toBe('unknown');
		expect(sends).toBe(3);
	});
	test('revoked notification authorization skips pending sends', async () => {
		configure(false);
		let sends = 0;
		await processIntegrityNotice(
			env,
			{
				reply: async () => {
					sends++;
				}
			} as unknown as QqClient,
			'12345'
		);
		expect(sends).toBe(0);
		expect((await queued())[3].state).toBe('skipped');
		configure();
	});
});
