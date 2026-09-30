import { describe, test, expect } from 'bun:test';
import { sql } from 'drizzle-orm';
import { testEnv, hasTestDb } from './db';
import { seedWorld } from './world';
import { callLoad, stubGateway } from './call';
import {
	organizations,
	servers,
	matches,
	integrityCases,
	integrityActions,
	siteSettings
} from '$lib/server/db/schema';
import { integrityActionHistory } from '$lib/server/integrity/action-history';
import { processIntegrityBatch } from '$lib/server/integrity/pipeline';
import { saveAssessmentMode } from '$lib/server/integrity/rules';
import { acquireOrRenew, releaseOwnership } from '$lib/server/leadership';
import type { KillView } from '$lib/types';
import { load } from '../routes/(app)/server/[id]/integrity/+page.server';
import { longModelEnabled, shortModelEnabled, committeeEnabled } from '$lib/integrity-engines';
import { retainedCase } from '$lib/server/integrity/case-retention';

describe.skipIf(!hasTestDb)('Selected integrity engines and unified punishment history', () => {
	test('unselected engines do not consume feed windows or create scores/cases; page omits unused data', async () => {
		const env = await testEnv(),
			w = await seedWorld(env);
		stubGateway();
		const request = new Request('http://localhost/test');
		expect(await acquireOrRenew(env, 'engine-gates-test')).toBe(true);
		const batch: KillView[] = Array.from({ length: 8 }, (_, i) => ({
			eventId: 'engine-' + i,
			instanceId: 'boot',
			matchId: 'match',
			ts: new Date().toISOString(),
			map: 'Kavkazi',
			eventTime: 100 + i,
			killer: { steamId: '76561198000000001', name: 'A', faction: 'Blue' },
			victim: { steamId: '76561198000000002', name: 'B', faction: 'Red' },
			cause: 'Id.Item.AK74M',
			distanceM: 10,
			headshot: true,
			suicide: false,
			teamKill: false,
			tags: []
		}));
		try {
			for (const mode of ['disabled', 'short_only'] as const) {
				await saveAssessmentMode(env, request, w.users.owner!, w.org.id, mode, '');
				await processIntegrityBatch(env, w.server.id, batch);
				for (const table of ['integrity_windows', 'integrity_scores', 'integrity_cases']) {
					const [count] = await env.db.execute(
						sql`SELECT count(*)::int n FROM ${sql.identifier(table)} WHERE server_id=${w.server.id}`
					);
					expect(count.n).toBe(0);
				}
				const data = (await callLoad(load, w.users.owner, { params: { id: w.server.id } }))
					.body as any;
				expect(data.onlinePlayers).toEqual([]);
				expect(data.scores).toEqual([]);
				expect(data.distributions.metrics).toEqual([]);
				expect(data).not.toHaveProperty('dryRun');
				expect(data.shortRisk === null).toBe(mode === 'disabled');
			}
		} finally {
			await releaseOwnership(env);
		}
	});
	test('both engines and each independent engine have separate enablement', () => {
		expect([
			shortModelEnabled('model_only'),
			longModelEnabled('model_only'),
			committeeEnabled('model_only')
		]).toEqual([true, true, false]);
		expect([shortModelEnabled('long_only'), longModelEnabled('long_only')]).toEqual([false, true]);
		expect([shortModelEnabled('short_only'), longModelEnabled('short_only')]).toEqual([
			true,
			false
		]);
	});
	test('cleanup removes ordinary cases but keeps vigilance and punishment-linked cases', async () => {
		const env = await testEnv(),
			w = await seedWorld(env),
			now = new Date(),
			ids = Array.from({ length: 4 }, () => crypto.randomUUID());
		for (let i = 0; i < 4; i++)
			await env.db.insert(integrityCases).values({
				id: ids[i],
				orgId: w.org.id,
				serverId: w.server.id,
				steamId: '76561198000000001',
				createdAt: now,
				confidence: 'B',
				trigger: 'test',
				ruleVersion: 1,
				riskScore: 60,
				riskBreakdown: [],
				snapshot: {},
				statistical: i === 0 ? null : { level: i === 1 ? 'NORMAL' : i === 2 ? 'WATCH' : 'CASE' }
			});
		await env.db.insert(integrityActions).values({
			id: crypto.randomUUID(),
			caseId: ids[3],
			orgId: w.org.id,
			serverId: w.server.id,
			steamId: '76561198000000001',
			action: 'KICK',
			source: 'RULE',
			deliveryState: 'delivered'
		});
		const deleted = await env.db
			.delete(integrityCases)
			.where(sql`server_id=${w.server.id} AND NOT ${retainedCase}`)
			.returning({ id: integrityCases.id });
		expect(deleted.map((c) => c.id).sort()).toEqual(ids.slice(0, 2).sort());
		const data = (await callLoad(load, w.users.owner, { params: { id: w.server.id } })).body as any;
		expect(data.cases.map((c: any) => c.id).sort()).toEqual(ids.slice(2).sort());
	});
	test('short warnings and confirmed/unknown kicks coexist with long model and administrator actions, isolated by server', async () => {
		const env = await testEnv(),
			w = await seedWorld(env);
		const steam = '76561198000000001',
			id = crypto.randomUUID(),
			now = new Date();
		await env.db.insert(integrityCases).values({
			id,
			orgId: w.org.id,
			serverId: w.server.id,
			steamId: steam,
			createdAt: now,
			confidence: 'B',
			trigger: 'test',
			ruleVersion: 1,
			riskScore: 90,
			riskBreakdown: [],
			snapshot: {}
		});
		await env.db.insert(integrityActions).values({
			id,
			caseId: id,
			orgId: w.org.id,
			serverId: w.server.id,
			steamId: steam,
			action: 'KICK',
			source: 'REVIEW',
			deliveryState: 'delivered',
			effectiveAt: now
		});
		const [match] = await env.db
			.insert(matches)
			.values({ serverId: w.server.id, map: 'A', startedAt: now })
			.returning();
		await env.db.execute(
			sql`INSERT INTO integrity_model_runs(id,org_id,server_id,steam_id,match_id,slot,config_revision,threshold,state,score,action,action_state,punished_at) VALUES(${id},${w.org.id},${w.server.id},${steam},${match.id},0,'test',0.03,'READY',0.04,'KICK','unknown',${now})`
		);
		for (const state of ['warning', 'delivered'])
			await env.db.insert(siteSettings).values({
				key: `shortRisk:${w.server.id}:${state}`,
				value: {
					serverId: w.server.id,
					steamId: steam,
					state,
					score: 0.7,
					name: 'short',
					reason: state,
					updatedAt: now.toISOString(),
					...(state === 'delivered' ? { attemptedAt: now.toISOString() } : {})
				}
			});
		const history = await integrityActionHistory(env, w.server.id);
		expect(history).toHaveLength(4);
		expect(history.find((a) => a.source === 'LONG_MODEL')?.deliveryState).toBe('unknown');
		expect(
			history
				.filter((a) => a.source === 'SHORT_MODEL')
				.map((a) => a.action)
				.sort()
		).toEqual(['KICK', 'WARNING']);
		expect(history.find((a) => a.source === 'REVIEW')?.effectiveAt).not.toBeNull();
		expect(await integrityActionHistory(env, w.otherOrgServer.id)).toEqual([]);
	});
});
