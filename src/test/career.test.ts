// A player's career is put together from sessions, matches and the kill feed; these check the
// joins between them against a database.
import { beforeAll, describe, expect, test } from 'bun:test';
import type { Env } from '$lib/server/env';
import { matches, playerSessions } from '$lib/server/db/schema';
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
});
