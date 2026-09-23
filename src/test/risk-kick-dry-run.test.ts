// The risk kick dry run replays everyone who was on the server in the last 24 hours, as the live
// rule judges whoever is on: a busy day's later joiners and a player on since before the window
// are both judged, not only the day's first 500 joins.
import { beforeAll, describe, expect, test } from 'bun:test';
import { eq } from 'drizzle-orm';
import type { Env } from '$lib/server/env';
import { playerMarks, playerSessions, servers } from '$lib/server/db/schema';
import { dryRun } from '$lib/server/triggers';
import { hasTestDb, testEnv } from './db';
import { seedWorld, type World } from './world';

const LATE = '76561198000000501';
const LONG = '76561198000000502';

describe.skipIf(!hasTestDb)('the risk kick dry run', () => {
	let env: Env;
	let w: World;

	beforeAll(async () => {
		// no Steam: a developer's .env may hold a real key, and the watchlist needs none
		env = { ...(await testEnv()), STEAM_API_KEY: '' };
		w = await seedWorld(env);
		const now = Date.now();
		const at = (msAgo: number) => new Date(now - msAgo);
		// 600 other joins through the day, then the watched player late in it
		await env.db.insert(playerSessions).values(
			Array.from({ length: 600 }, (_, i) => ({
				serverId: w.server.id,
				steamId: `7656119800001${String(i).padStart(4, '0')}`,
				name: `filler${i}`,
				joinedAt: at(20 * 3600_000 - i * 60_000),
				lastSeen: at(20 * 3600_000 - i * 60_000 - 30_000),
				leftAt: at(20 * 3600_000 - i * 60_000 - 30_000)
			}))
		);
		await env.db.insert(playerSessions).values([
			{
				serverId: w.server.id,
				steamId: LATE,
				name: 'Late',
				joinedAt: at(3600_000),
				lastSeen: at(3000_000),
				leftAt: at(3000_000)
			},
			// on since two days ago and still on
			{
				serverId: w.server.id,
				steamId: LONG,
				name: 'Long',
				joinedAt: at(48 * 3600_000),
				lastSeen: at(10_000)
			}
		]);
		await env.db.insert(playerMarks).values([
			{ orgId: w.org.id, steamId: LATE, watched: true, reason: 'note' },
			{ orgId: w.org.id, steamId: LONG, watched: true, reason: 'note' }
		]);
	});

	test('finds a match past the 500th join and one on since before the window', async () => {
		const [server] = await env.db.select().from(servers).where(eq(servers.id, w.server.id));
		const r = await dryRun(env, server, 'risk_kick', { watchlist: true, reason: 'no' });
		expect(r.fires).toBe(2);
		expect(r.items.map((i) => i.text).sort()).toEqual([
			`kick Late (${LATE}): on the watchlist (note)`,
			`kick Long (${LONG}): on the watchlist (note)`
		]);
		expect(r.notes).toContain(
			'602 distinct players on the server in the window, each judged once.'
		);
	});
});
