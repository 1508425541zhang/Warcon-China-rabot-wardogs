import { describe, expect, test } from 'bun:test';
import { refreshIntegrityBaselines, selectWeaponBaselines } from '$lib/server/integrity/baselines';
import { integrityBaselines, kills } from '$lib/server/db/schema';
import { eq } from 'drizzle-orm';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';

describe.skipIf(!hasTestDb)('weapon baseline refresh', () => {
	test('the database builds exact-weapon cohorts without borrowing another weapon', async () => {
		const env = await testEnv();
		const world = await seedWorld(env);
		const at = new Date(Date.now() - 20 * 60_000);
		for (let start = 0; start < 200; start += 20) {
			await env.db.insert(kills).values(
				Array.from({ length: 200 }, (_, n) => {
					const player = start + Math.floor(n / 10);
					const kill = n % 10;
					return {
						ts: at,
						serverId: world.server.id,
						eventId: `weapon-${player}-${kill}`,
						instanceId: 'weapon-test',
						matchId: 'round-1',
						eventTime: 10 + kill,
						map: 'Kavkazi',
						killerSteamId: String(76561198000000000n + BigInt(player)),
						killerFaction: 'Blue',
						victimSteamId: String(76561198100000000n + BigInt(n)),
						victimName: 'Target',
						victimFaction: 'Red',
						cause: 'Id.Item.AK74M',
						distanceM: 100 + player,
						headshot: kill % 2 === 0,
						tags: []
					};
				})
			);
		}
		expect(await refreshIntegrityBaselines(env, world.org.id)).toBeGreaterThan(0);
		const rows = await env.db
			.select()
			.from(integrityBaselines)
			.where(eq(integrityBaselines.orgId, world.org.id));
		const selected = selectWeaponBaselines(rows, 'Kavkazi', null);
		expect(selected.get('headshotRateWeapon:Id.Item.AK74M')?.sampleCount).toBe(200);
		expect(selected.get('maxKillDistanceWeapon:Id.Item.AK74M')?.sampleCount).toBe(200);
		expect(selected.has('headshotRateWeapon:Id.Item.SVDM')).toBe(false);
	});
});
