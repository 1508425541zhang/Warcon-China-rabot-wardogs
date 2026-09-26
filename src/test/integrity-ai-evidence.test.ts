import { describe, test, expect } from 'bun:test';
import { compactTable, expandedAiEvidence } from '$lib/server/integrity/ai-evidence';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';
import { integrityCases, kills, matches } from '$lib/server/db/schema';

function decode(t: ReturnType<typeof compactTable>) {
	return t.rows.map((row) =>
		Object.fromEntries(
			t.columns.flatMap((c, i) => {
				const v = row[i];
				if (v && typeof v === 'object' && '$missing' in v) return [];
				return [[c, v !== null && t.dictionaries[c] ? t.dictionaries[c][v as number] : v]];
			})
		)
	);
}
test('column dictionaries round-trip IDs, numeric distances, null, missing, tags and unknown fields', () => {
	const records = [
		{
			id: '76561198000000000',
			distance: 0,
			faction: null,
			tags: ['Penetration'],
			extra: { x: true }
		},
		{ id: '76561198000000000', distance: 120, faction: 'A', tags: [] }
	];
	expect(decode(compactTable(records))).toEqual(records);
	expect(compactTable(records).dictionaries.id).toHaveLength(1);
});
describe.skipIf(!hasTestDb)('AI complete combat evidence', () => {
	test('includes >1000 subject events, deaths, environment, post-case; isolates other rounds and servers', async () => {
		const env = await testEnv(),
			w = await seedWorld(env),
			now = new Date();
		const [round] = await env.db
			.insert(matches)
			.values({ serverId: w.server.id, map: 'Map', startedAt: new Date(now.getTime() - 3600000) })
			.returning();
		const [other] = await env.db
			.insert(matches)
			.values({ serverId: w.server.id, map: 'Map', startedAt: new Date(now.getTime() - 7200000) })
			.returning();
		const subject = '76561198000000001',
			otherPlayer = '76561198000000002';
		const make = (id: string, extra: Partial<typeof kills.$inferInsert> = {}) => ({
			serverId: w.server.id,
			ts: now,
			eventId: id,
			instanceId: 'boot',
			matchId: 'game-round',
			matchRow: round.id,
			eventTime: 100,
			map: 'Map',
			killerSteamId: subject,
			victimSteamId: otherPlayer,
			victimName: 'other',
			cause: 'Id.Item.AK74M',
			tags: [],
			...extra
		});
		const anchor = make('anchor', { eventTime: 1000 });
		await env.db
			.insert(kills)
			.values([
				anchor,
				...Array.from({ length: 1001 }, (_, i) => make('early-' + i)),
				make('death', { killerSteamId: otherPlayer, victimSteamId: subject }),
				make('environment', { killerSteamId: null, victimSteamId: subject }),
				make('background', {
					killerSteamId: otherPlayer,
					victimSteamId: '76561198000000003',
					eventTime: 950
				}),
				make('unrelated', { killerSteamId: otherPlayer, victimSteamId: '76561198000000003' }),
				make('late', { ts: new Date(now.getTime() + 1000), eventTime: 1100 }),
				make('wrong-round', { matchRow: other.id }),
				make('wrong-server', { serverId: 'not-this-server' })
			]);
		const [c] = await env.db
			.insert(integrityCases)
			.values({
				id: crypto.randomUUID(),
				orgId: w.org.id,
				serverId: w.server.id,
				steamId: subject,
				createdAt: now,
				confidence: 'B',
				trigger: 'test',
				ruleVersion: 1,
				riskScore: 40,
				riskBreakdown: [],
				snapshot: { instanceId: 'boot', clockTo: 1000 }
			})
			.returning();
		const saved = [
			{
				instanceId: 'boot',
				eventId: 'anchor',
				event: { eventId: 'anchor', eventTime: 1000, custom: { keep: true } }
			}
		];
		const evidence = await expandedAiEvidence(env, c, saved),
			rows = decode(evidence.combatEvents),
			ids = rows.map((r) => r.eventId);
		expect(rows).toHaveLength(1006);
		expect(ids).toContain('death');
		expect(ids).toContain('environment');
		expect(ids).toContain('background');
		expect(ids).toContain('late');
		expect(ids).not.toContain('unrelated');
		expect(ids).not.toContain('wrong-round');
		expect(ids).not.toContain('wrong-server');
		expect(evidence.counts.subjectDeaths).toBe(2);
		expect(evidence.counts.postCaseRecords).toBe(1);
		expect(evidence.coverage.nonFatalDamage.available).toBe(false);
		expect(evidence.coverage.nonFatalDamage.events).toBeNull();
		expect(decode(evidence.frozenEvents.data)).toEqual(saved.map((r) => r.event));
		const fallback = await expandedAiEvidence(env, { ...c, snapshot: {} }, []);
		expect(fallback.scope.kind).toBe('unresolved_round');
		expect(decode(fallback.combatEvents).map((r) => r.eventId)).not.toContain('background');
	});
});
