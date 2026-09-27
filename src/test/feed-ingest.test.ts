// The kill feed against a real database: the game sends a batch again when it did not hear back,
// and the copy can arrive while the first is still being written.
import { beforeAll, describe, expect, test } from 'bun:test';
import { randomUUID } from 'node:crypto';
import { and, eq, sql } from 'drizzle-orm';
import type { Env } from '$lib/server/env';
import { kills, trainingFeedBatches } from '$lib/server/db/schema';
import { ingestBatch } from '$lib/server/feed';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';

const batchOf = (eventIds: string[]) => ({
	serverId: 'e9cf2544-b21e-4b80-9f12-8ec95ff58964',
	serverName: 'Test',
	events: eventIds.map((eventId, i) => ({
		eventId,
		type: 'killed',
		eventTime: 100 + i,
		matchId: '7e72e869-4460-4d27-aa4c-76a52ed20cb4',
		mapName: 'Kavkazi',
		killerName: 'Alpha',
		killerSteamId: '76561198000000001',
		victimName: 'Bravo',
		victimSteamId: '76561198000000002',
		cause: 'Id.Item.AK74M',
		distance: 70,
		contextTags: []
	}))
});

describe.skipIf(!hasTestDb)('kill feed ingest', () => {
	let env: Env;

	beforeAll(async () => {
		env = await testEnv();
	});

	const stored = async (serverId: string, eventId: string) =>
		(
			await env.db
				.select({ id: kills.eventId })
				.from(kills)
				.where(and(eq(kills.serverId, serverId), eq(kills.eventId, eventId)))
		).length;

	test('a batch that arrives many times at once is written once', async () => {
		const w = await seedWorld(env);
		const ids = [randomUUID(), randomUUID()];
		const results = await Promise.all(
			Array.from({ length: 12 }, () => ingestBatch(env, w.server.id, batchOf(ids)))
		);
		expect(results.reduce((n, r) => n + r.accepted, 0)).toBe(2);
		expect(results.reduce((n, r) => n + r.duplicates, 0)).toBe(22);
		for (const id of ids) expect(await stored(w.server.id, id)).toBe(1);
		const receipts = await env.db
			.select()
			.from(trainingFeedBatches)
			.where(eq(trainingFeedBatches.serverId, w.server.id));
		expect(receipts.length).toBe(12);
	});

	test('preserves unknown events, nested fields, exact tags, missing flags and source clocks', async () => {
		const w = await seedWorld(env);
		const body = batchOf([randomUUID()]);
		const first = {
			...body.events[0],
			eventTime: 123.123456789,
			distance: 25000.123456,
			contextTags: [
				'Meta.Progression.Context.Player.KillContext.Headshot',
				'Meta.Progression.Context.Player.KillContext.Penetration',
				'Future.Flag'
			],
			damage: { amount: 42.25, bone: 'head' },
			killerId: 'raw-game-id'
		};
		const payload = {
			...body,
			futureVersion: 17,
			events: [
				first,
				{ eventId: randomUUID(), type: 'damaged', amount: 7.5 },
				{ ...body.events[0], eventId: randomUUID(), contextTags: undefined },
				null,
				'future-event'
			]
		};
		// HTTP JSON serialization omits undefined fields.
		const wire = JSON.parse(JSON.stringify(payload));
		const received = new Date('2026-09-28T12:00:00.123Z');
		const result = await ingestBatch(env, w.server.id, wire, received);
		expect(result.accepted).toBe(2);
		expect(result.skipped).toBe(3);
		const [receipt] = await env.db
			.select()
			.from(trainingFeedBatches)
			.where(eq(trainingFeedBatches.serverId, w.server.id));
		expect(receipt.payload).toEqual(wire);
		expect(receipt.receivedAt.toISOString()).toBe(received.toISOString());
		const rows = await env.db.execute(
			sql`SELECT * FROM training_feed_events WHERE batch_id=${receipt.id} ORDER BY event_index`
		);
		expect(rows.length).toBe(5);
		expect(rows[0].weapon).toBe('Id.Item.AK74M');
		expect(rows[0].headshot).toBe(true);
		expect(rows[0].penetration).toBe(true);
		expect(Number(rows[0].event_time_seconds)).toBe(first.eventTime);
		expect(Number(rows[0].raw_distance_cm)).toBe(first.distance);
		expect(rows[1].event_type).toBe('damaged');
		expect(rows[2].headshot).toBeNull();
		expect(rows[2].penetration).toBeNull();
	});

	test('keeps non-kill-only and empty envelopes', async () => {
		const w = await seedWorld(env);
		for (const events of [[], [{ type: 'knocked', eventId: 'KO-1', health: null }]]) {
			const result = await ingestBatch(env, w.server.id, {
				serverId: 'boot',
				serverName: 'Test',
				events
			});
			expect(result.accepted).toBe(0);
		}
		expect(
			(
				await env.db
					.select()
					.from(trainingFeedBatches)
					.where(eq(trainingFeedBatches.serverId, w.server.id))
			).length
		).toBe(2);
	});

	test('archive and kill roll back together when downstream job persistence fails', async () => {
		const w = await seedWorld(env);
		const eventId = randomUUID();
		await env.db.execute(
			sql.raw(
				`ALTER TABLE feed_processing_jobs ADD CONSTRAINT training_test_reject CHECK (server_id <> '${w.server.id}')`
			)
		);
		try {
			await expect(ingestBatch(env, w.server.id, batchOf([eventId]))).rejects.toThrow();
			expect(await stored(w.server.id, eventId)).toBe(0);
			expect(
				(
					await env.db
						.select()
						.from(trainingFeedBatches)
						.where(eq(trainingFeedBatches.serverId, w.server.id))
				).length
			).toBe(0);
		} finally {
			await env.db.execute(
				sql`ALTER TABLE feed_processing_jobs DROP CONSTRAINT training_test_reject`
			);
		}
	});

	test("the same event id on another server is that server's own kill", async () => {
		const w = await seedWorld(env);
		const id = randomUUID();
		const [a, b] = await Promise.all([
			ingestBatch(env, w.server.id, batchOf([id])),
			ingestBatch(env, w.otherServer.id, batchOf([id]))
		]);
		expect([a.accepted, b.accepted]).toEqual([1, 1]);
	});
});
