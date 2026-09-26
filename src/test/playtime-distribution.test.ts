import { describe, expect, test } from 'bun:test';
import { summarizePlaytime } from '../lib/server/playtime-distribution';
import { parsePlaytime, WARDOGS_APP_ID } from '../lib/server/steam-playtime';
test('unknown excluded; tied minutes use weighted nearest-rank P80', () => {
	const result = summarizePlaytime(
		[
			{ minutes: null, state: 'pending', count: 100 },
			{ minutes: 60, state: 'known', count: 8 },
			{ minutes: 120, state: 'known', count: 2 }
		],
		true
	);
	expect(result.total).toBe(110);
	expect(result.available).toBe(10);
	expect(result.p80Minutes).toBe(60);
	expect(result.bins.reduce((s, b) => s + b.count, 0)).toBe(10);
	expect(result.bins.at(-1)?.cumulative).toBe(100);
});
test('zero is known; missing is not zero; boundaries are half open', () => {
	const r = summarizePlaytime(
		[
			{ minutes: 0, state: 'known', count: 1 },
			{ minutes: 60, state: 'known', count: 1 }
		],
		true
	);
	expect(r.bins.map((b) => b.count)).toEqual([1, 1]);
	expect(
		summarizePlaytime([{ minutes: null, state: 'unavailable', count: 2 }], true).p80Minutes
	).toBeNull();
});
test('Steam parser accepts only matching app and valid minutes', () => {
	const body = (appid: number, playtime_forever: unknown) => ({
		response: { games: [{ appid, playtime_forever }] }
	});
	expect(parsePlaytime(body(WARDOGS_APP_ID, 0))).toBe(0);
	for (const v of [-1, '10', Infinity, 1.2, 2147483648])
		expect(parsePlaytime(body(WARDOGS_APP_ID, v))).toBeNull();
	expect(parsePlaytime(body(1, 100))).toBeNull();
	expect(parsePlaytime({ response: {} })).toBeNull();
});
test('extreme values keep chart bounded', () => {
	expect(
		summarizePlaytime([{ minutes: 2147483647, state: 'known', count: 1 }], true).bins.length
	).toBeLessThanOrEqual(20);
});

import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';
import { playerSessions, steamGamePlaytime } from '../lib/server/db/schema';
import { loadPlaytimeDistribution } from '../lib/server/steam-playtime';
describe.skipIf(!hasTestDb)('playtime cohort database', () => {
	test('deduplicates sessions, scopes server and date, excludes stale and missing samples', async () => {
		const env = await testEnv();
		const w = await seedWorld(env);
		const now = new Date();
		const old = new Date(Date.now() - 3 * 86400000);
		const ids = [1, 2, 3, 4, 5].map((n) => `7656119800000990${n}`);
		await env.db
			.insert(playerSessions)
			.values([
				...[0, 0, 1, 2].map((i) => ({
					serverId: w.server.id,
					steamId: ids[i],
					name: 'player',
					joinedAt: old,
					lastSeen: now
				})),
				{
					serverId: w.otherServer.id,
					steamId: ids[3],
					name: 'other',
					joinedAt: old,
					lastSeen: now
				},
				{ serverId: w.server.id, steamId: ids[4], name: 'old', joinedAt: old, lastSeen: old }
			]);
		await env.db.insert(steamGamePlaytime).values([
			{ steamId: ids[0], minutes: 120, state: 'known', checkedAt: now },
			{ steamId: ids[1], minutes: 600, state: 'known', checkedAt: old },
			{ steamId: ids[3], minutes: 999, state: 'known', checkedAt: now }
		]);
		const r = await loadPlaytimeDistribution(env, w.server.id, new Date(Date.now() - 86400000));
		expect(r.total).toBe(3);
		expect(r.available).toBe(1);
		expect(r.pending).toBe(2);
		expect(r.p80Minutes).toBe(120);
	});
});
