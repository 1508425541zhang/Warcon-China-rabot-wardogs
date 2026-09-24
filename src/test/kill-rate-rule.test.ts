// A Kill rate rule as the worker runs it: kills from the feed reach the rule on their own server
// only, only hand-held weapon kills count, and a flag is an outbox row that sends nothing to the game.
import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import { eq } from 'drizzle-orm';
import type { Env } from '$lib/server/env';
import { kills, outbox, triggers } from '$lib/server/db/schema';
import { acquireOrRenew, releaseOwnership } from '$lib/server/leadership';
import { onKillsIngested } from '$lib/server/feed-events';
import { dryRun } from '$lib/server/triggers';
import { KILL_RATE_FLAG } from '$lib/server/kill-rate';
import { newId } from '$lib/server/http';
import type { KillView } from '$lib/types';
import { hasTestDb, testEnv } from './db';
import { seedWorld, type World } from './world';

const FAST = '76561198000000401';
const DRIVER = '76561198000000402';
const VICTIM = '76561198000000403';
const UNLUCKY = '76561198000000404';
const STEADY = '76561198000000405';
const BURST = '76561198000000406';

const kill = (killer: string, cause: string, i: number): KillView => ({
	eventId: newId(),
	ts: new Date(Date.now() + i).toISOString(),
	map: 'Kavkazi',
	eventTime: i,
	killer: { steamId: killer, name: `p${killer.slice(-3)}`, faction: null },
	victim: { steamId: VICTIM, name: 'victim', faction: null },
	cause,
	distanceM: 50,
	headshot: false,
	suicide: false,
	teamKill: false,
	tags: []
});

describe.skipIf(!hasTestDb)('Kill rate rule, live', () => {
	let env: Env;
	let w: World;
	let here: string;
	let there: string;

	const rule = async (serverId: string) => {
		const id = newId();
		await env.db.insert(triggers).values({
			id,
			serverId,
			orgId: w.org.id,
			kind: 'kill_rate',
			name: 'Kill rate watch',
			enabled: true,
			config: {
				windowMinutes: 5,
				maxKills: 3,
				headshotPct: 0,
				headshotMinKills: 15,
				cooldownMinutes: 30
			}
		});
		return id;
	};
	const rowsOf = (triggerId: string) =>
		env.db.select().from(outbox).where(eq(outbox.triggerId, triggerId));

	beforeAll(async () => {
		env = await testEnv();
		w = await seedWorld(env);
		expect(await acquireOrRenew(env, 'kill-rate test')).toBe(true);
		here = await rule(w.server.id);
		there = await rule(w.otherServer.id);
	});
	afterAll(() => releaseOwnership(env));

	test('three quick rifle kills flag the player once, as a flag and nothing else', async () => {
		await onKillsIngested(
			env,
			w.server.id,
			[0, 1, 2, 3].map((i) => kill(FAST, 'Id.Item.AK74M', i))
		);
		const rows = await rowsOf(here);
		expect(rows.length).toBe(1);
		expect(rows[0]).toMatchObject({
			serverId: w.server.id,
			action: KILL_RATE_FLAG,
			target: FAST,
			steamId: FAST,
			state: 'pending'
		});
		expect(rows[0].detail).toMatchObject({ verdict: '3 kills in 5 min' });
		// the other server's rule saw none of it
		expect((await rowsOf(there)).length).toBe(0);
	});

	test('vehicle kills are not counted', async () => {
		await onKillsIngested(
			env,
			w.server.id,
			[0, 1, 2, 3, 4].map((i) => kill(DRIVER, 'Id.Vehicle.WeaponExtension.STN_02.MainCannon', i))
		);
		const rows = await rowsOf(here);
		expect(rows.map((r) => r.target)).toEqual([FAST]);
	});
	test('a flag that could not be queued does not start the cooldown', async () => {
		await releaseOwnership(env);
		await onKillsIngested(
			env,
			w.server.id,
			[0, 1, 2].map((i) => kill(UNLUCKY, 'Id.Item.AK74M', i))
		);
		expect((await rowsOf(here)).map((r) => r.target)).not.toContain(UNLUCKY);
		expect(await acquireOrRenew(env, 'kill-rate test')).toBe(true);
		await onKillsIngested(env, w.server.id, [kill(UNLUCKY, 'Id.Item.AK74M', 3)]);
		expect((await rowsOf(here)).map((r) => r.target)).toContain(UNLUCKY);
	});

	test('the dry run spaces a held-back batch out by the match clock', async () => {
		const server = { ...w.server, name: 'one' } as Parameters<typeof dryRun>[1];
		const received = new Date();
		const row = (killer: string, eventTime: number) => ({
			ts: received,
			serverId: w.server.id,
			eventId: newId(),
			instanceId: 'i',
			matchId: 'm',
			eventTime,
			map: 'Kavkazi',
			killerSteamId: killer,
			killerName: killer.slice(-3),
			victimSteamId: VICTIM,
			victimName: 'victim',
			cause: 'Id.Item.AK74M',
			tags: []
		});
		// one batch: STEADY's four kills six minutes apart, BURST's three within a minute
		await env.db
			.insert(kills)
			.values([
				...[0, 360, 720, 1080].map((t) => row(STEADY, t)),
				...[1100, 1120, 1140].map((t) => row(BURST, t))
			]);
		const r = await dryRun(env, server, 'kill_rate', { maxKills: 3, windowMinutes: 5 });
		const who = r.items.map((i) => i.text);
		expect(who.some((t) => t.includes(BURST))).toBe(true);
		expect(who.some((t) => t.includes(STEADY))).toBe(false);
	});
});
