// Migration 0032 gives the matches played before match_players existed their rows, from the
// closed sessions that overlapped them and the kills that carry them. It runs once at deploy on
// an empty table; here its statements run again over a seeded world, which they may (rows the
// worker wrote are left alone, the feed columns are recomputed).
import { beforeAll, describe, expect, test } from 'bun:test';
import { asc, eq, sql } from 'drizzle-orm';
import type { Env } from '$lib/server/env';
import { kills, matches, matchPlayers, playerSessions } from '$lib/server/db/schema';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';

const A = '76561198000000071';
const B = '76561198000000072';
const C = '76561198000000073';
const D = '76561198000000074';
const X = '76561198000000075';
const HOUR = 3_600_000;
const MIN = 60_000;

async function runBackfill(env: Env) {
	const text = await Bun.file('drizzle/0032_match_players_backfill.sql').text();
	for (const stmt of text.split('--> statement-breakpoint')) await env.db.execute(sql.raw(stmt));
}

describe.skipIf(!hasTestDb)('the match rows backfill', () => {
	let env: Env;
	let serverId: string;
	let m1 = 0;
	let m2 = 0;
	let m3 = 0;
	const t0 = Date.now() - 10 * HOUR;
	const at = (ms: number) => new Date(t0 + ms);

	beforeAll(async () => {
		env = await testEnv();
		const w = await seedWorld(env);
		serverId = w.server.id;
		const scores = [
			{ name: 'Lonestar', score: 100 },
			{ name: 'Wagner', score: 40 }
		];
		const rows = await env.db
			.insert(matches)
			.values([
				{
					serverId,
					startedAt: at(0),
					endedAt: at(HOUR),
					map: 'Europe',
					finalScores: scores,
					winner: 'Lonestar'
				},
				{
					serverId,
					startedAt: at(2 * HOUR),
					endedAt: at(3 * HOUR),
					map: 'Kavkazi',
					finalScores: scores,
					winner: 'Lonestar'
				},
				{ serverId, startedAt: at(4 * HOUR), map: 'Zestafona' }
			])
			.returning({ id: matches.id });
		[m1, m2, m3] = rows.map((r) => r.id);
		const session = (
			steamId: string,
			from: number,
			to: number | null,
			faction: string,
			k: number,
			d: number,
			name = steamId
		) => ({
			serverId,
			steamId,
			name,
			faction,
			joinedAt: at(from),
			lastSeen: at(to ?? from + 5 * MIN),
			leftAt: to === null ? null : at(to),
			kills: k,
			deaths: d,
			cash: 1000
		});
		await env.db.insert(playerSessions).values([
			// A: one session inside the first match, then a short rejoin with the counters kept
			session(A, 15 * MIN, 45 * MIN, 'Lonestar', 10, 2, 'A'),
			session(A, 50 * MIN, 55 * MIN, 'Lonestar', 12, 2, 'A (later)'),
			// B: one session across the end of the first match and into the second
			session(B, 30 * MIN, 2 * HOUR + 30 * MIN, 'Wagner', 20, 5),
			// C: still on: the worker's
			session(C, 4 * HOUR + MIN, null, 'Wagner', 7, 1),
			// D: left during the match still open
			session(D, 4 * HOUR + 5 * MIN, 4 * HOUR + 20 * MIN, 'Lonestar', 3, 4)
		]);
		const kill = (
			eventTime: number,
			killer: string | null,
			victim: string,
			extra: Partial<typeof kills.$inferInsert> = {}
		): typeof kills.$inferInsert => ({
			ts: at(eventTime * 1000),
			serverId,
			eventId: `bf${eventTime}`,
			instanceId: 'i',
			matchId: 'g1',
			matchRow: m1,
			eventTime,
			map: 'Europe',
			killerSteamId: killer,
			killerName: killer,
			killerFaction: 'Lonestar',
			victimSteamId: victim,
			victimName: victim,
			victimFaction: 'Wagner',
			cause: 'Id.Item.AK74M',
			distanceM: 30,
			headshot: false,
			suicide: false,
			teamKill: false,
			tags: [],
			...extra
		});
		await env.db
			.insert(kills)
			.values([
				kill(1000, A, B, { headshot: true, distanceM: 150 }),
				kill(1100, A, B, { cause: 'Vehicle.Variant.Land.Wheeled.Kodiak.Pickup', distanceM: 4 }),
				kill(1200, B, A),
				kill(1300, A, A, { suicide: true, cause: null, distanceM: null }),
				kill(1400, A, X, { teamKill: true }),
				kill(1500, null, A, { cause: null, distanceM: null })
			]);
		await runBackfill(env);
	});

	const rowsOf = (matchId: number) =>
		env.db
			.select()
			.from(matchPlayers)
			.where(eq(matchPlayers.matchId, matchId))
			.orderBy(asc(matchPlayers.steamId));

	test('a session gives every match it overlapped a row; the counters go to the last one', async () => {
		const first = await rowsOf(m1);
		expect(first.map((r) => [r.steamId, r.name, r.faction, r.seconds, r.kills, r.deaths])).toEqual([
			[A, 'A (later)', 'Lonestar', 35 * 60, 12, 2],
			[B, B, 'Wagner', 30 * 60, 0, 0]
		]);
		const second = await rowsOf(m2);
		expect(second.map((r) => [r.steamId, r.seconds, r.kills, r.deaths, r.cashDelta])).toEqual([
			[B, 30 * 60, 20, 5, 0]
		]);
	});

	test('the match still open gets its leavers; a session still open is left to the worker', async () => {
		const third = await rowsOf(m3);
		expect(third.map((r) => [r.steamId, r.seconds, r.kills])).toEqual([[D, 15 * 60, 3]]);
	});

	test('the feed columns are filled for the rows that exist, and a player the feed alone saw gets none', async () => {
		const first = await rowsOf(m1);
		const a = first.find((r) => r.steamId === A)!;
		expect([
			a.headshots,
			a.vehicleKills,
			a.longestM,
			a.teamKills,
			a.suicides,
			a.killStreak,
			a.deathStreak
		]).toEqual([1, 1, 150, 1, 1, 2, 3]);
		const b = first.find((r) => r.steamId === B)!;
		expect([b.killStreak, b.deathStreak, b.headshots]).toEqual([1, 2, 0]);
		expect(first.some((r) => r.steamId === X)).toBe(false);
	});

	test('running it again changes nothing', async () => {
		const before = await rowsOf(m1);
		await runBackfill(env);
		expect(await rowsOf(m1)).toEqual(before);
	});
});
