import { describe, expect, test } from 'bun:test';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';
import { matches } from '$lib/server/db/schema';
import { recordPlayerProgress, loadPlayerProgress } from '$lib/server/player-progress';
import type { Player } from '$lib/types';
import { eq } from 'drizzle-orm';
describe.skipIf(!hasTestDb)('persistent player money samples', () => {
	test('deduplicates time bucket, keeps matches separate and retrieves real progression', async () => {
		const env = await testEnv();
		const w = await seedWorld(env);
		const start = new Date(Date.now() - 120000);
		const [m] = await env.db
			.insert(matches)
			.values({
				serverId: w.server.id,
				startedAt: start,
				map: 'Europe',
				experiences: 'KOTH',
				lighting: 'Day',
				peakPlayers: 2
			})
			.returning();
		const p: Player = {
			steamId: '76561198000990001',
			name: 'one',
			faction: 'A',
			kills: 0,
			deaths: 0,
			cash: 100,
			ping: 10
		};
		await recordPlayerProgress(env.db, w.server.id, [p], start);
		await recordPlayerProgress(env.db, w.server.id, [p], start);
		await recordPlayerProgress(
			env.db,
			w.server.id,
			[{ ...p, cash: 170 }],
			new Date(start.getTime() + 60000)
		);
		const data = await loadPlayerProgress(env, w.server.id, p.steamId);
		expect(data).toHaveLength(1);
		expect(data[0].id).toBe(m.id);
		expect(data[0].points.map((v) => v.growth)).toEqual([0, 70]);
		expect(await loadPlayerProgress(env, 'other-server', p.steamId)).toEqual([]);
		await env.db.update(matches).set({ endedAt: new Date() }).where(eq(matches.id, m.id));
		await recordPlayerProgress(env.db, w.server.id, [{ ...p, cash: 900 }], new Date());
		expect((await loadPlayerProgress(env, w.server.id, p.steamId))[0].points).toHaveLength(2);
	});
});
