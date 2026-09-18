// The ban on sight works from a list the worker keeps in memory between syncs. What it bans must
// still be wanted when the player turns up.
import { beforeAll, describe, expect, test } from 'bun:test';
import { eq } from 'drizzle-orm';
import type { Env } from '$lib/server/env';
import { listEntries, organizations, servers } from '$lib/server/db/schema';
import { ensureOrgLists, ensureServerLists, grantEntry, listOf } from '$lib/server/lists';
import { banOnSight } from '$lib/server/lists-sync';
import type { RefusedBan } from '$lib/server/lists-plan';
import type { WardogsClient } from '$lib/server/rcon';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';

const STEAM = '76561198000000042';

describe.skipIf(!hasTestDb)('ban on sight', () => {
	let env: Env;

	beforeAll(async () => {
		env = await testEnv();
	});

	/** A server whose org bans STEAM, the game having refused the ban while the player was away. */
	async function refusedBan(expiresAt: Date | null = null) {
		const w = await seedWorld(env);
		await ensureOrgLists(env.db, w.org.id);
		await ensureServerLists(env.db, w.server.id, w.org.id);
		const list = await listOf(env, w.org.id, 'ban');
		const entry = await grantEntry(env, list, {
			steamId: STEAM,
			reason: 'cheating',
			expiresAt,
			addedByName: 'test'
		});
		const [server] = await env.db.select().from(servers).where(eq(servers.id, w.server.id));
		const [org] = await env.db.select().from(organizations).where(eq(organizations.id, w.org.id));
		const refused = new Map<string, RefusedBan>([
			[STEAM, { steamId: STEAM, reason: 'cheating', listId: list.id }]
		]);
		const sent: string[] = [];
		const client = {
			json: async (method: string, path: string) => {
				sent.push(`${method} ${path}`);
				return {};
			}
		} as unknown as WardogsClient;
		return { server, org, refused, sent, client, entryId: entry.id };
	}

	test('a wanted ban is placed when the player is seen', async () => {
		const t = await refusedBan();
		await banOnSight(env, t.server, t.org, t.client, [STEAM], t.refused);
		expect(t.sent).toEqual(['POST /v1/bans']);
		expect(t.refused.size).toBe(0);
	});

	test('a ban taken off the list since the last sync is not placed', async () => {
		const t = await refusedBan();
		await env.db
			.update(listEntries)
			.set({ removedAt: new Date(), removal: 'manual' })
			.where(eq(listEntries.id, t.entryId));
		await banOnSight(env, t.server, t.org, t.client, [STEAM], t.refused);
		expect(t.sent).toEqual([]);
		expect(t.refused.size).toBe(0);
	});

	test('a ban that ran out since the last sync is not placed', async () => {
		const t = await refusedBan(new Date(Date.now() + 60_000));
		await env.db
			.update(listEntries)
			.set({ expiresAt: new Date(Date.now() - 1000) })
			.where(eq(listEntries.id, t.entryId));
		await banOnSight(env, t.server, t.org, t.client, [STEAM], t.refused);
		expect(t.sent).toEqual([]);
		expect(t.refused.size).toBe(0);
	});
});
