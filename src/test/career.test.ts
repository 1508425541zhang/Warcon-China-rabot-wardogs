// A player's career is put together from sessions, matches and the kill feed; these check the
// joins between them against a database.
import { beforeAll, describe, expect, test } from 'bun:test';
import type { Env } from '$lib/server/env';
import { kills, matches, playerSessions } from '$lib/server/db/schema';
import { loadBoard, loadCareer } from '$lib/server/leaderboards';
import { parseBoardQuery } from '$lib/leaderboard';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';

const STEAM = '76561198000000061';
const HOUR = 3_600_000;

describe.skipIf(!hasTestDb)('career', () => {
	let env: Env;

	beforeAll(async () => {
		env = await testEnv();
	});

	test('a session that ended on the holding team has no result in its matches', async () => {
		const w = await seedWorld(env);
		const t0 = Date.now() - 10 * HOUR;
		const scores = [
			{ name: 'Lonestar', score: 100 },
			{ name: 'Wagner', score: 40 }
		];
		await env.db.insert(matches).values([
			{
				serverId: w.server.id,
				startedAt: new Date(t0),
				endedAt: new Date(t0 + HOUR),
				map: 'Europe',
				finalScores: scores,
				winner: 'Lonestar'
			},
			{
				serverId: w.server.id,
				startedAt: new Date(t0 + 2 * HOUR),
				endedAt: new Date(t0 + 3 * HOUR),
				map: 'Kavkazi',
				finalScores: scores,
				winner: 'Lonestar'
			}
		]);
		const session = (from: number, faction: string) => ({
			serverId: w.server.id,
			steamId: STEAM,
			name: 'ARTEC',
			faction,
			joinedAt: new Date(t0 + from),
			lastSeen: new Date(t0 + from + HOUR / 2),
			leftAt: new Date(t0 + from + HOUR / 2)
		});
		await env.db
			.insert(playerSessions)
			.values([session(HOUR / 4, 'White'), session(2 * HOUR + HOUR / 4, 'Wagner')]);

		const career = await loadCareer(env, {
			serverId: w.server.id,
			ids: [w.server.id],
			nameOf: new Map(),
			steamId: STEAM
		});
		expect(career).toMatchObject({ matches: 2, wins: 0, losses: 1, draws: 0 });
		expect(career.last.map((m) => [m.faction, m.result])).toEqual([
			['Wagner', 'loss'],
			['White', null]
		]);

		const board = await loadBoard(
			env,
			[w.server.id],
			parseBoardQuery(new URLSearchParams('minMinutes=0'))
		);
		const row = board.rows.find((r) => r.steamId === STEAM);
		expect(row).toMatchObject({ matches: 2, wins: 0, losses: 1, draws: 0 });
	});

	test('kills stored under the name players know join the map the match was on', async () => {
		const w = await seedWorld(env);
		const t0 = Date.now() - 5 * HOUR;
		const [match] = await env.db
			.insert(matches)
			.values({
				serverId: w.server.id,
				startedAt: new Date(t0),
				endedAt: new Date(t0 + HOUR),
				map: 'NorthAmerica',
				finalScores: [
					{ name: 'Lonestar', score: 100 },
					{ name: 'Wagner', score: 40 }
				],
				winner: 'Lonestar'
			})
			.returning({ id: matches.id });
		await env.db.insert(playerSessions).values({
			serverId: w.server.id,
			steamId: STEAM,
			name: 'ARTEC',
			faction: 'Lonestar',
			joinedAt: new Date(t0 + 1000),
			lastSeen: new Date(t0 + HOUR),
			leftAt: new Date(t0 + HOUR)
		});
		await env.db.insert(kills).values(
			['a', 'b'].map((n) => ({
				ts: new Date(t0 + 60_000),
				serverId: w.server.id,
				eventId: `${w.server.id}-${n}`,
				instanceId: 'i',
				matchId: 'm',
				matchRow: match.id,
				eventTime: 1,
				map: 'Zestafona',
				killerSteamId: STEAM,
				killerName: 'ARTEC',
				killerFaction: 'Lonestar',
				victimSteamId: '76561198000000062',
				victimName: 'other',
				victimFaction: 'Wagner',
				tags: []
			}))
		);
		const career = await loadCareer(env, {
			serverId: w.server.id,
			ids: [w.server.id],
			nameOf: new Map(),
			steamId: STEAM
		});
		expect(career.maps).toEqual([
			{ key: 'NorthAmerica', matches: 1, wins: 1, losses: 0, draws: 0, kills: 2, deaths: 0 }
		]);
	});
});
