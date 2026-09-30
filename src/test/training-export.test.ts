import { describe, expect, test } from 'bun:test';
import { createHash } from 'node:crypto';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve, sep } from 'node:path';
import { trainingFeedBatches, trainingObservations } from '$lib/server/db/schema';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';

describe.skipIf(!hasTestDb)('training source JSONL export', () => {
	test('exports only selected server, preserves missing counters, verifies hashes and refuses overwrite', async () => {
		const env = await testEnv();
		const w = await seedWorld(env);
		const at = new Date();
		const payload = {
			players: [{ steamId: '76561198000000001', health: null, future: { weapon: 'AK74M' } }]
		};
		await env.db.insert(trainingObservations).values({
			serverId: w.server.id,
			pollStartedAt: at,
			receivedAt: at,
			endpoint: '/v1/players',
			payload
		});
		await env.db.insert(trainingFeedBatches).values([
			{
				serverId: w.server.id,
				instanceId: 'boot',
				receivedAt: at,
				payload: { events: [{ type: 'damage', amount: 7.25 }] }
			},
			{ serverId: w.otherServer.id, instanceId: 'other', receivedAt: at, payload: { events: [] } }
		]);
		const folder = await mkdtemp(join(tmpdir(), 'warcon-training-export-'));
		try {
			const destination = join(folder, 'export');
			const run = () =>
				Bun.spawn([process.execPath, 'scripts/export-training-data.ts', w.server.id, destination], {
					stdout: 'pipe',
					stderr: 'pipe',
					env: process.env
				});
			const child = run();
			const error = await new Response(child.stderr).text();
			expect(await child.exited, error).toBe(0);
			const manifest = JSON.parse(await readFile(join(destination, 'manifest.json'), 'utf8'));
			expect(manifest.files.map((f: any) => f.rows)).toEqual([1, 1]);
			for (const f of manifest.files) {
				const bytes = await readFile(join(destination, f.name));
				expect(createHash('sha256').update(bytes).digest('hex')).toBe(f.sha256);
			}
			const record = JSON.parse(
				(await readFile(join(destination, 'training_observations.jsonl'), 'utf8')).trim()
			);
			expect(record.payload).toEqual(payload);
			expect(record.payload.players[0]).not.toHaveProperty('kills');
			expect(await run().exited).not.toBe(0);
		} finally {
			if (!resolve(folder).startsWith(resolve(tmpdir()) + sep))
				throw new Error('Unsafe test cleanup path');
			await rm(folder, { recursive: true, force: true });
		}
	});
});
