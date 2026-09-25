import { describe, expect, test } from 'bun:test';
import { eq } from 'drizzle-orm';
import { integrityBaselines, integrityImportBatches, kills } from '$lib/server/db/schema';
import { refreshIntegrityBaselines, selectBaselines } from '$lib/server/integrity/baselines';
import {
	parseExternalHistory,
	reviewIntegrityImport,
	stageIntegrityImport
} from '$lib/server/integrity/imports';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';

const event = (i: number, date: Date) => ({
	eventId: `external-${i}`,
	eventAt: new Date(date.getTime() + i * 180_000).toISOString(),
	instanceId: 'startup-1',
	matchId: 'round-1',
	eventTime: i * 180 + 5,
	map: 'Kavkazi',
	killerSteamId: i % 2 ? '76561198000000888' : '76561198000000889',
	victimSteamId: '76561198000000999',
	killerFaction: 'Blue',
	victimFaction: 'Red',
	cause: 'Id.Item.AK74M',
	distanceM: 50 + i,
	headshot: i % 2 === 0,
	penetration: false,
	playerCount: 42
});

describe('external Integrity JSON/JSONL validation', () => {
	test('accepts JSON array and JSONL with explicit units and rejects duplicate or ambiguous rows', () => {
		const date = new Date(Date.now() - 86_400_000);
		const one = event(1, date);
		expect(parseExternalHistory(JSON.stringify([one]))).toHaveLength(1);
		expect(
			parseExternalHistory(`${JSON.stringify(one)}\n${JSON.stringify(event(2, date))}`)
		).toHaveLength(2);
		expect(() => parseExternalHistory(JSON.stringify([one, one]))).toThrow(/eventId/);
		expect(() => parseExternalHistory(JSON.stringify([{ ...one, distanceM: '5000' }]))).toThrow(
			/distanceM/
		);
		expect(() => parseExternalHistory(JSON.stringify([{ ...one, distanceM: 5001 }]))).toThrow(
			/distanceM/
		);
		expect(() =>
			parseExternalHistory(
				JSON.stringify([{ ...one, eventAt: new Date(Date.now() - 40 * 86_400_000).toISOString() }])
			)
		).toThrow(/eventAt/);
		expect(() => parseExternalHistory(JSON.stringify([{ ...one, killerFaction: 'Red' }]))).toThrow(
			/同阵营/
		);
	});
});

describe.skipIf(!hasTestDb)('approved external Integrity baselines', () => {
	test('staging does not affect live kills; approval enables fallback and revocation removes it', async () => {
		const env = await testEnv();
		const world = await seedWorld(env);
		const now = new Date(Date.now() - 86_400_000);
		const raw = Array.from({ length: 40 }, (_, i) => JSON.stringify(event(i, now))).join('\n');
		const request = new Request('http://localhost/test', { method: 'POST' });
		const batch = await stageIntegrityImport(
			env,
			request,
			world.users.owner!,
			world.org.id,
			'outside-01',
			raw
		);
		expect(batch.rowCount).toBe(40);
		await expect(refreshIntegrityBaselines(env, world.org.id)).rejects.toThrow(
			/No eligible clean baseline/
		);
		expect(
			await env.db.select().from(kills).where(eq(kills.serverId, world.server.id))
		).toHaveLength(0);
		await reviewIntegrityImport(
			env,
			request,
			world.users.owner!,
			world.org.id,
			batch.id,
			'APPROVED'
		);
		const rows = await env.db
			.select()
			.from(integrityBaselines)
			.where(eq(integrityBaselines.orgId, world.org.id));
		const baseline = selectBaselines(rows, 'Kavkazi', '41–60').get('kpm180');
		expect(baseline?.source).toBe('external');
		expect(baseline?.sampleCount).toBe(40);
		await reviewIntegrityImport(
			env,
			request,
			world.users.owner!,
			world.org.id,
			batch.id,
			'REJECTED'
		);
		expect(
			await env.db
				.select()
				.from(integrityBaselines)
				.where(eq(integrityBaselines.orgId, world.org.id))
		).toHaveLength(0);
		const [stored] = await env.db
			.select()
			.from(integrityImportBatches)
			.where(eq(integrityImportBatches.id, batch.id));
		expect(stored.status).toBe('REJECTED');
	});
});
