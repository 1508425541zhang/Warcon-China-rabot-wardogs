// The match broadcast dry run replays a map change as the live rule sees it: the game shows
// nobody on while the next map loads, so the announcement waits for the players to be back.
import { beforeAll, describe, expect, test } from 'bun:test';
import { eq } from 'drizzle-orm';
import type { Env } from '$lib/server/env';
import { samples, servers } from '$lib/server/db/schema';
import { dryRun } from '$lib/server/triggers';
import { hasTestDb, testEnv } from './db';
import { seedWorld, type World } from './world';

describe.skipIf(!hasTestDb)('the match broadcast dry run', () => {
	let env: Env;
	let w: World;

	beforeAll(async () => {
		env = await testEnv();
		w = await seedWorld(env);
		const t0 = Date.now() - 3600_000;
		const at = (s: number) => new Date(t0 + s * 1000);
		const scores = (a: number, b: number) => [
			{ name: 'Valkyra', score: a },
			{ name: 'Lonestar', score: b }
		];
		await env.db.insert(samples).values(
			[
				{ ts: at(0), playerCount: 63, map: 'Bakurani', scores: scores(90, 70) },
				{ ts: at(20), playerCount: 63, map: 'Bakurani', scores: scores(100, 81) },
				// the next map, nobody on yet
				{ ts: at(40), playerCount: 0, map: 'Madrid', scores: scores(0, 0) },
				{ ts: at(60), playerCount: 0, map: 'Madrid', scores: scores(0, 0) },
				{ ts: at(80), playerCount: 61, map: 'Madrid', scores: scores(0, 0) },
				{ ts: at(100), playerCount: 62, map: 'Madrid', scores: scores(2, 1) }
			].map((r) => ({ ...r, serverId: w.server.id, ok: true }))
		);
	});

	test('announces the match that ended once the players are back, once', async () => {
		const [server] = await env.db.select().from(servers).where(eq(servers.id, w.server.id));
		const r = await dryRun(env, server, 'match_broadcast', {
			endMessage: '{faction} won on {previous}',
			startMessage: 'Next: {map}',
			minPlayers: 1
		});
		expect(r.items.map((i) => i.text)).toEqual([
			'broadcast (61 on): Valkyra won on Bakurani',
			'broadcast (61 on): Next: Madrid'
		]);
	});
});
