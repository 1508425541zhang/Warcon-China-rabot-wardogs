import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import { eq } from 'drizzle-orm';
import type { KillView } from '$lib/types';
import type { Env } from '$lib/server/env';
import {
	integrityCases,
	integrityScores,
	integrityWindows,
	kills,
	organizations,
	outbox,
	servers,
	steamProfiles
} from '$lib/server/db/schema';
import { onKillsIngested } from '$lib/server/feed-events';
import { waitIntegrityBatch } from '$lib/server/integrity/pipeline';
import { acquireOrRenew, releaseOwnership } from '$lib/server/leadership';
import { forgetMemory, memoryFor } from '$lib/server/observe';
import { hasTestDb, testEnv } from './db';
import { seedWorld, type World } from './world';

describe.skipIf(!hasTestDb)('Community Integrity behavior persistence', () => {
	let env: Env;
	let world: World;
	const player = '76561198000000777';
	const steamPlayer = '76561198000000778';
	const overlapPlayer = '76561198000000779';
	const instanceId = 'integrity-behavior-boot';

	const event = (steamId: string, clock: number, id: string, headshot = false): KillView => ({
		eventId: id,
		instanceId,
		matchId: 'match-1',
		ts: new Date().toISOString(),
		map: 'Kavkazi',
		eventTime: clock,
		killer: { steamId, name: 'Player', faction: 'Blue' },
		victim: {
			steamId: `765611980000${String(10000 + clock).slice(-5)}`,
			name: 'Target',
			faction: 'Red'
		},
		cause: 'Id.Item.AK74M',
		distanceM: 20,
		headshot,
		suicide: false,
		teamKill: false,
		tags: []
	});
	const send = async (batch: KillView[]) => {
		await env.db.insert(kills).values(
			batch.map((item) => ({
				ts: new Date(item.ts),
				serverId: world.server.id,
				eventId: item.eventId,
				instanceId: item.instanceId!,
				matchId: item.matchId!,
				eventTime: item.eventTime,
				map: item.map,
				killerSteamId: item.killer!.steamId,
				killerName: item.killer!.name,
				killerFaction: item.killer!.faction,
				victimSteamId: item.victim.steamId,
				victimName: item.victim.name,
				victimFaction: item.victim.faction,
				cause: item.cause,
				headshot: item.headshot,
				suicide: item.suicide,
				teamKill: item.teamKill,
				tags: item.tags
			}))
		);
		await onKillsIngested(env, world.server.id, batch);
		await waitIntegrityBatch(world.server.id);
	};

	beforeAll(async () => {
		env = await testEnv();
		world = await seedWorld(env);
		const [[server], [org]] = await Promise.all([
			env.db.select().from(servers).where(eq(servers.id, world.server.id)),
			env.db.select().from(organizations).where(eq(organizations.id, world.org.id))
		]);
		memoryFor(server, org);
		expect(await acquireOrRenew(env, 'integrity behavior test')).toBe(true);
	});
	afterAll(async () => {
		forgetMemory(world.server.id);
		await releaseOwnership(env);
	});

	test('upgrades one active window, freezes the full case signal set and takes no action', async () => {
		await send(Array.from({ length: 12 }, (_, i) => event(player, i * 12, `slow-${i}`)));
		let windows = await env.db
			.select()
			.from(integrityWindows)
			.where(eq(integrityWindows.steamId, player));
		expect(windows).toHaveLength(1);
		expect(windows[0].kpm180).toBe(4);
		await send([event(player, 135, 'slow-extra')]);
		expect(
			await env.db.select().from(integrityScores).where(eq(integrityScores.steamId, player))
		).toHaveLength(1);
		await send(Array.from({ length: 8 }, (_, i) => event(player, 140 + i * 2, `fast-${i}`, true)));
		windows = await env.db
			.select()
			.from(integrityWindows)
			.where(eq(integrityWindows.steamId, player));
		expect(windows).toHaveLength(1);
		expect(windows[0].eventIds).toHaveLength(21);
		const scores = await env.db
			.select()
			.from(integrityScores)
			.where(eq(integrityScores.steamId, player));
		expect(scores).toHaveLength(2);
		expect(scores[1].windowId).toBe(scores[0].windowId);
		expect(scores[1].score).toBeGreaterThan(scores[0].score);
		const [evidence] = await env.db
			.select()
			.from(integrityCases)
			.where(eq(integrityCases.steamId, player));
		expect(evidence).toBeDefined();
		const snapshot = evidence.snapshot as Record<string, unknown>;
		expect(snapshot.behaviorReasons).toContain('burst');
		expect(snapshot.burstPoints).toBe(12);
		expect(snapshot.infantryKills).toBe(21);
		expect(snapshot.uniqueVictims).toBe(21);
		expect(snapshot.headshots).toBe(8);
		expect(snapshot.headshotPct).toBeCloseTo((8 / 21) * 100);
		expect(snapshot.penetrations).toBe(0);
		expect(snapshot.penetrationPct).toBe(0);
		expect(snapshot.steamBansKnown).toBe(false);
		expect(snapshot.vacBans).toBe(0);
		expect(snapshot.gameBans).toBe(0);
		expect(snapshot.daysSinceLastBan).toBeNull();
		expect(snapshot.repeatAutoKo).toBe(false);
		expect(snapshot.eventIds).toHaveLength(21);
		expect(snapshot.ruleVersion).toBe(1);
		expect(await env.db.select().from(outbox).where(eq(outbox.steamId, player))).toHaveLength(0);
	});

	test('a cached private Steam profile supplies VAC data only after a behavior finding', async () => {
		await env.db.insert(steamProfiles).values({
			steamId: steamPlayer,
			public: false,
			persona: 'Private',
			avatar: '',
			profileUrl: '',
			vacBans: 1,
			gameBans: 0,
			daysSinceLastBan: 10,
			fetchedAt: new Date(),
			error: ''
		});
		const before = env.STEAM_API_KEY;
		env.STEAM_API_KEY = 'cached-profile-test-key';
		try {
			await send(Array.from({ length: 8 }, (_, i) => event(steamPlayer, i * 2, `steam-${i}`)));
		} finally {
			env.STEAM_API_KEY = before;
		}
		const [score] = await env.db
			.select()
			.from(integrityScores)
			.where(eq(integrityScores.steamId, steamPlayer));
		const [window] = await env.db
			.select()
			.from(integrityWindows)
			.where(eq(integrityWindows.steamId, steamPlayer));
		expect(window.kpm180).toBeLessThan(4);
		expect(score.currentBehaviorAnomaly).toBe(true);
		expect(
			(score.breakdown as { code: string; points: number }[]).find(
				(part) => part.code === 'steam_ban_prior'
			)?.points
		).toBe(8);
	});

	test('after Worker memory loss overlapping saved evidence cannot earn repeat-window points', async () => {
		const now = new Date();
		const ids = Array.from({ length: 12 }, (_, i) => `restart-${i}`);
		const [old] = await env.db
			.insert(integrityWindows)
			.values({
				orgId: world.org.id,
				serverId: world.server.id,
				steamId: overlapPlayer,
				instanceId,
				map: 'Kavkazi',
				clockFrom: 0,
				clockTo: 132,
				observedAt: new Date(now.getTime() - 60_000),
				infantryKills: 12,
				kpm180: 4,
				uniqueVictims: 12,
				eventIds: ids
			})
			.returning();
		await env.db.insert(integrityScores).values({
			windowId: old.id,
			orgId: world.org.id,
			serverId: world.server.id,
			steamId: overlapPlayer,
			scoredAt: new Date(now.getTime() - 60_000),
			ruleVersion: 1,
			score: 54,
			level: 'AUTO_KO',
			breakdown: [],
			currentBehaviorAnomaly: true
		});
		await send(
			Array.from({ length: 12 }, (_, i) =>
				event(overlapPlayer, i * 12, i === 11 ? 'restart-new' : ids[i])
			)
		);
		const windows = await env.db
			.select()
			.from(integrityWindows)
			.where(eq(integrityWindows.steamId, overlapPlayer));
		expect(windows).toHaveLength(1);
		expect(windows[0].id).toBe(old.id);
		const scores = await env.db
			.select()
			.from(integrityScores)
			.where(eq(integrityScores.steamId, overlapPlayer));
		expect(scores).toHaveLength(2);
		expect(scores[1].windowId).toBe(old.id);
		expect(scores[1].breakdown).not.toContainEqual(
			expect.objectContaining({ code: 'repeat_window' })
		);
		expect(scores[1].breakdown).not.toContainEqual(
			expect.objectContaining({ code: 'repeat_auto_ko' })
		);
	});
});
