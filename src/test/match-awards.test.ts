import { describe, expect, test } from 'bun:test';
import { testEnv, hasTestDb } from './db';
import { seedWorld } from './world';
import { matches, kills } from '$lib/server/db/schema';
import { loadAwardLines } from '$lib/server/match-awards';
import { awardWinners } from '$lib/match-awards-policy';
describe.skipIf(!hasTestDb)('round award snapshot', () => {
	test('old round only, deaths reset streak, same timestamp multi, duplicate dedupe and stable snapshot', async () => {
		const env = await testEnv(),
			world = await seedWorld(env),
			now = new Date();
		const [round] = await env.db
			.insert(matches)
			.values({
				serverId: world.server.id,
				map: 'Europe',
				startedAt: new Date(now.getTime() - 600000)
			})
			.returning();
		const event = (id: string, time: number, killer = 'a', victim = 'b') => ({
			serverId: world.server.id,
			ts: now,
			eventId: id,
			instanceId: 'i',
			matchId: 'game',
			matchRow: round.id,
			map: 'Europe',
			eventTime: time,
			killerSteamId: killer,
			killerName: killer,
			victimSteamId: victim,
			victimName: victim,
			tags: []
		});
		await env.db
			.insert(kills)
			.values([
				event('1', 10),
				event('2', 10, 'a', 'c'),
				event('3', 11, 'b', 'a'),
				event('4', 12),
				event('1', 10),
				{ ...event('other', 13), matchRow: round.id + 1 }
			]);
		const memory = [
			{ name: '甲', steamId: 'a', kills: 3, deaths: 1, cashHeld: 100, cashDelta: 50, seconds: 600 },
			{ name: '乙', steamId: 'b', kills: 3, deaths: 1, cashHeld: 200, cashDelta: 10, seconds: 600 }
		];
		const first = await loadAwardLines(env, world.server.id, 'Europe', memory);
		expect(first.find((p) => p.steamId === 'a')?.streak).toBe(2);
		expect(first.find((p) => p.steamId === 'a')?.multi).toBe(2);
		expect(first.find((p) => p.steamId === 'a')?.cashHeld).toBe(100);
		const retry = await loadAwardLines(
			env,
			world.server.id,
			'Europe',
			memory.map((p) => ({ ...p, kills: 900, cashHeld: 9000 }))
		);
		expect(retry).toEqual(first);
		expect(awardWinners(retry)).toEqual(awardWinners(first));
		expect(await loadAwardLines(env, world.server.id, 'Different', memory)).toEqual([]);
	});
});
