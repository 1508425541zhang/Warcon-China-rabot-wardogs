import { beforeAll, describe, expect, test, spyOn } from 'bun:test';
import { sql } from 'drizzle-orm';
import { testEnv, hasTestDb } from './db';
import { seedWorld, type World } from './world';
import { callApi, stubGateway } from './call';
import type { Env } from '$lib/server/env';
import { GET, PUT } from '../routes/api/orgs/[id]/integrity/model/+server';
import {
	modelConfigView,
	saveModelConfig,
	modelUrl,
	CALIBRATION_SHA,
	MODEL_ID,
	MODEL_SHA,
	MODEL_SCHEMA,
	validateModelResult
} from '$lib/server/integrity/model-http';
import {
	runPlayerModel,
	processModelQueue,
	modelSources,
	scheduleOnlineModels
} from '$lib/server/integrity/model-runtime';
import { saveAssessmentMode } from '$lib/server/integrity/rules';
import { processIntegrityBatch } from '$lib/server/integrity/pipeline';
import { acquireOrRenew, releaseOwnership } from '$lib/server/leadership';
import type { KillView } from '$lib/types';
import {
	enforceModelRun,
	modelDecision,
	modelDeliverySkipReason
} from '$lib/server/integrity/model-enforcement';
import { MODEL_CALIBRATION } from '$lib/server/integrity/model-http';
import { memoryFor, forgetMemory } from '$lib/server/observe';
import { getServer, getOrg } from '$lib/server/access';
import { outbox, listEntries } from '$lib/server/db/schema';
import { eq } from 'drizzle-orm';
import { saveVips, vipSettings } from '$lib/server/qq/vip';
import { desiredFor } from '$lib/server/lists-sync';

test('model identity and finite scores are mandatory; redirects and credential URLs are not configuration', () => {
	expect(() => modelUrl('http://user:secret@localhost')).toThrow();
	expect(() => modelUrl('file:///etc/passwd')).toThrow();
	expect(modelUrl('http://127.0.0.1:8091/')).toBe('http://127.0.0.1:8091');
	const result = {
		modelId: MODEL_ID,
		checkpointSha256: MODEL_SHA,
		calibrationSha256: CALIBRATION_SHA,
		schema: MODEL_SCHEMA,
		requestId: 'r',
		status: 'READY',
		score: 0.1,
		windowSeconds: 1800,
		bucketSeconds: 30,
		pointScores: Array(60).fill(0.1)
	};
	expect(validateModelResult(result, 'r').status).toBe('READY');
	expect(() => validateModelResult({ ...result, score: NaN }, 'r')).toThrow();
	expect(() => validateModelResult({ ...result, checkpointSha256: 'wrong' }, 'r')).toThrow();
	expect(() => validateModelResult(result, 'other-request')).toThrow();
	expect(() =>
		validateModelResult(
			{ ...result, windowSeconds: 1800, bucketSeconds: 30, pointScores: Array(200).fill(0.1) },
			'r'
		)
	).toThrow();
	expect(modelDecision(MODEL_CALIBRATION.p97 - 1e-8)).toBeNull();
	expect(modelDecision(MODEL_CALIBRATION.p97)).toBe('KICK');
	expect(MODEL_CALIBRATION.p97).toBeGreaterThan(MODEL_CALIBRATION.p95);
	expect(MODEL_CALIBRATION.p97).toBeLessThan(MODEL_CALIBRATION.p99);
	expect(modelDecision(MODEL_CALIBRATION.p95)).toBeNull();
	expect(modelDecision((MODEL_CALIBRATION.p95 + MODEL_CALIBRATION.p97) / 2)).toBeNull();
	expect(modelDecision(MODEL_CALIBRATION.p99)).toBe('QUARANTINE_24H');
});

describe.skipIf(!hasTestDb)('A测 HTTP model', () => {
	let env: Env, w: World, matchId: number;
	const steam = '76561198000000001';
	beforeAll(async () => {
		env = await testEnv();
		w = await seedWorld(env);
		stubGateway();
	}, 120000);
	test('owner-only settings, encrypted token, conflict checking and developer gate', async () => {
		const params = { id: w.org.id };
		expect((await callApi(GET, null, { params })).status).toBe(401);
		expect((await callApi(PUT, w.users.member, { params, method: 'PUT', body: {} })).status).toBe(
			403
		);
		await expect(
			saveAssessmentMode(
				env,
				new Request('http://localhost'),
				w.users.admin!,
				w.org.id,
				'model_only',
				''
			)
		).rejects.toThrow();
		const config = await saveModelConfig(env, w.org.id, w.users.admin!.id, {
			revision: 'empty',
			developerEnabled: true,
			url: 'http://127.0.0.1:8091',
			token: 'a'.repeat(32),
			threshold: 0.05,
			intervalSeconds: 1800
		});
		expect(config.hasToken).toBe(true);
		expect(JSON.stringify(config)).not.toContain('a'.repeat(32));
		await expect(
			saveModelConfig(env, w.org.id, w.users.admin!.id, { ...config, revision: 'empty' })
		).rejects.toThrow();
		await saveAssessmentMode(
			env,
			new Request('http://localhost'),
			w.users.admin!,
			w.org.id,
			'model_only',
			''
		);
	});
	test('source isolation, durable dedupe, response and error handling', async () => {
		const [match] = await env.db.execute<{ id: number }>(
			sql`INSERT INTO matches(server_id,started_at) VALUES(${w.server.id},now()-interval '2 hours') RETURNING id`
		);
		matchId = Number(match.id);
		await env.db.execute(
			sql`INSERT INTO player_progress_samples(server_id,match_id,bucket,observed_at,players) VALUES(${w.server.id},${matchId},1,now()-interval '30 seconds',${JSON.stringify(
				[
					{ steamId: steam, cash: 100, kills: 2, deaths: 1, name: 'private' },
					{ steamId: '76561198000000002', cash: 500 }
				]
			)}::text::jsonb)`
		);
		const sources = await modelSources(env, w.server.id, steam, matchId, new Date());
		expect(JSON.stringify(sources)).not.toContain(steam);
		expect(JSON.stringify(sources)).not.toContain('private');
		expect(Number(sources.player_progress_samples[0].roster_size)).toBe(2);
		await expect(modelSources(env, 'other-server', steam, matchId, new Date())).rejects.toThrow();
		await runPlayerModel(env, w.org.id, w.server.id, steam, matchId);
		await runPlayerModel(env, w.org.id, w.server.id, steam, matchId);
		const count = await env.db.execute(
			sql`SELECT count(*) AS n FROM integrity_model_runs WHERE server_id=${w.server.id}`
		);
		expect(Number(count[0].n)).toBe(1);
		const mocked = spyOn(globalThis, 'fetch').mockImplementation((async (
			_url: RequestInfo | URL,
			init?: RequestInit
		) => {
			const body = JSON.parse(String(init?.body));
			return new Response(
				JSON.stringify({
					modelId: MODEL_ID,
					checkpointSha256: MODEL_SHA,
					calibrationSha256: CALIBRATION_SHA,
					schema: MODEL_SCHEMA,
					requestId: body.requestId,
					status: 'READY',
					score: 0.2,
					windowSeconds: 1800,
					bucketSeconds: 30,
					pointScores: Array(60).fill(0.2)
				})
			);
		}) as unknown as typeof fetch);
		try {
			await processModelQueue(env);
		} finally {
			mocked.mockRestore();
		}
		const [row] = await env.db.execute(
			sql`SELECT state,score FROM integrity_model_runs WHERE server_id=${w.server.id}`
		);
		expect(row.state).toBe('READY');
		expect(Number(row.score)).toBe(0.2);
		const config = await modelConfigView(env, w.org.id);
		await saveModelConfig(env, w.org.id, w.users.admin!.id, { ...config, threshold: 0.1 });
		await runPlayerModel(env, w.org.id, w.server.id, steam, matchId);
		const broken = spyOn(globalThis, 'fetch').mockImplementation((async () => {
			throw new Error('private internal secret');
		}) as unknown as typeof fetch);
		try {
			await processModelQueue(env);
		} finally {
			broken.mockRestore();
		}
		const errors = await env.db.execute(
			sql`SELECT result FROM integrity_model_runs WHERE server_id=${w.server.id} AND state='ERROR'`
		);
		expect(errors.length).toBe(1);
		expect(JSON.stringify(errors)).not.toContain('private internal');
	});
	test('model-only pipeline retains measurements and queues without legacy or expert verdicts', async () => {
		expect(await acquireOrRenew(env, 'model-alpha-test')).toBe(true);
		const batch: KillView[] = Array.from({ length: 5 }, (_, i) => ({
			eventId: `model-${i}`,
			instanceId: 'boot',
			matchId: 'match',
			matchRow: matchId,
			ts: new Date(Date.now() - 1000 + i * 10).toISOString(),
			map: 'Kavkazi',
			eventTime: 600 + i,
			killer: { steamId: steam, name: 'player', faction: 'Blue' },
			victim: { steamId: `7656119800000000${i + 2}`, name: 'target', faction: 'Red' },
			cause: 'Id.Item.AK74M',
			distanceM: 10,
			headshot: true,
			suicide: false,
			teamKill: false,
			tags: []
		}));
		try {
			await processIntegrityBatch(env, w.server.id, batch);
			const [windows] = await env.db.execute(
				sql`SELECT count(*) AS n FROM integrity_windows WHERE server_id=${w.server.id}`
			);
			expect(Number(windows.n)).toBeGreaterThan(0);
			await processIntegrityBatch(env, w.server.id, batch);
			const [again] = await env.db.execute(
				sql`SELECT count(*) AS n FROM integrity_windows WHERE server_id=${w.server.id}`
			);
			expect(Number(again.n)).toBe(Number(windows.n));
			for (const table of ['integrity_scores', 'integrity_cases', 'integrity_actions']) {
				const [count] = await env.db.execute(
					sql`SELECT count(*) AS n FROM ${sql.identifier(table)} WHERE server_id=${w.server.id}`
				);
				expect(Number(count.n)).toBe(0);
			}
		} finally {
			await releaseOwnership(env);
		}
	});
	test('online periodic sweep schedules players without kills and skips a young match', async () => {
		const m = memoryFor((await getServer(env, w.server.id))!, (await getOrg(env, w.org.id))!);
		m.ok = true;
		m.playersAt = m.statusAt = Date.now();
		m.players = [{ steamId: '76561198000000991', name: 'no kills' }] as typeof m.players;
		try {
			await scheduleOnlineModels(env);
			await scheduleOnlineModels(env);
			const [count] = await env.db.execute(
				sql`SELECT count(*) AS n FROM integrity_model_runs WHERE steam_id='76561198000000991'`
			);
			expect(Number(count.n)).toBe(1);
			const [young] = await env.db.execute<{ id: number }>(
				sql`INSERT INTO matches(server_id,started_at) VALUES(${w.server.id},now()-interval '1 minute') RETURNING id`
			);
			await runPlayerModel(env, w.org.id, w.server.id, '76561198000000992', Number(young.id));
			const [none] = await env.db.execute(
				sql`SELECT count(*) AS n FROM integrity_model_runs WHERE steam_id='76561198000000992'`
			);
			expect(Number(none.n)).toBe(0);
			await env.db.execute(sql`DELETE FROM matches WHERE id=${young.id}`);
		} finally {
			forgetMemory(w.server.id);
		}
	});
	test('P97 kicks, P99 creates a 24h ban, dedupe and late VIP exemption preserve manual bans', async () => {
		const server = (await getServer(env, w.server.id))!,
			org = (await getOrg(env, w.org.id))!;
		const m = memoryFor(server, org);
		m.ok = true;
		m.playersAt = m.statusAt = Date.now();
		const ids = ['76561198000000101', '76561198000000102'];
		m.players = ids.map((steamId) => ({ steamId, name: 'model test' })) as typeof m.players;
		expect(await acquireOrRenew(env, 'model-enforcement-test')).toBe(true);
		const config = await modelConfigView(env, w.org.id);
		const makeRun = async (index: number, score: number) => {
			const id = crypto.randomUUID();
			const result = {
				modelId: MODEL_ID,
				checkpointSha256: MODEL_SHA,
				calibrationSha256: CALIBRATION_SHA,
				schema: MODEL_SCHEMA,
				requestId: id,
				status: 'READY',
				score,
				windowSeconds: 1800,
				bucketSeconds: 30,
				pointScores: Array(60).fill(score)
			};
			await env.db.execute(
				sql`INSERT INTO integrity_model_runs(id,org_id,server_id,steam_id,match_id,slot,config_revision,threshold,state,score,result) VALUES(${id},${w.org.id},${w.server.id},${ids[index]},${matchId},${index},${config.revision},${MODEL_CALIBRATION.p97},'READY',${score},${JSON.stringify(result)}::text::jsonb)`
			);
			return id;
		};
		try {
			const kick = await makeRun(0, MODEL_CALIBRATION.p97);
			await enforceModelRun(env, kick);
			await enforceModelRun(env, kick);
			const [k] = await env.db.execute(
				sql`SELECT action,list_entry_id FROM integrity_model_runs WHERE id=${kick}`
			);
			expect(k.action).toBe('KICK');
			expect(k.list_entry_id).toBeNull();
			const quarantine = await makeRun(1, MODEL_CALIBRATION.p99);
			await enforceModelRun(env, quarantine);
			await enforceModelRun(env, quarantine);
			const [q] = await env.db.execute(
				sql`SELECT action,list_entry_id,expires_at,punished_at FROM integrity_model_runs WHERE id=${quarantine}`
			);
			expect(q.action).toBe('QUARANTINE_24H');
			expect(q.list_entry_id).toBeTruthy();
			expect(
				Math.abs(
					new Date(q.expires_at as string).getTime() -
						new Date(q.punished_at as string).getTime() -
						86400000
				)
			).toBeLessThan(1000);
			const messages = await env.db.select().from(outbox).where(eq(outbox.serverId, w.server.id));
			expect(messages.filter((x) => x.triggerKind === 'model_integrity').length).toBe(2);
			const msg = messages.find((x) => x.steamId === ids[1])!;
			expect(await modelDeliverySkipReason(env, msg)).toBeNull();
			const previous = await vipSettings(env);
			await saveVips(
				env,
				{
					revision: previous.revision,
					entries: [
						...previous.entries,
						{
							serverId: w.server.id,
							steamId: ids[1],
							enabled: true,
							reserve: false,
							allowOverkill: false,
							whitelist: true,
							note: ''
						}
					]
				},
				w.users.site!.id
			);
			expect(await modelDeliverySkipReason(env, msg)).toContain('VIP');
			expect((await desiredFor(env, server, org)).bans.some((x) => x.steamId === ids[1])).toBe(
				false
			);
			await env.db
				.update(listEntries)
				.set({ addedBy: w.users.site!.id, addedByName: 'manual' })
				.where(eq(listEntries.id, String(q.list_entry_id)));
			expect((await desiredFor(env, server, org)).bans.some((x) => x.steamId === ids[1])).toBe(
				true
			);
			await env.db
				.update(listEntries)
				.set({ expiresAt: new Date(Date.now() - 1000) })
				.where(eq(listEntries.id, String(q.list_entry_id)));
			expect((await desiredFor(env, server, org)).bans.some((x) => x.steamId === ids[1])).toBe(
				false
			);
		} finally {
			forgetMemory(w.server.id);
			await releaseOwnership(env);
		}
	});
});
