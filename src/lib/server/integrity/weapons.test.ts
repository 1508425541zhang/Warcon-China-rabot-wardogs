import { describe, expect, test } from 'bun:test';
import { classifyWeapon, countsAsInfantry } from './weapons';

const smallArm = {
	cause: 'Id.Item.AK74M',
	tags: [] as string[],
	suicide: false,
	killerSteamId: '76561198000000001',
	victimSteamId: '76561198000000002',
	killerFaction: 'Blue',
	victimFaction: 'Red'
};

describe('exact weapon classification', () => {
	test('known small arm counts only across known opposing factions', () => {
		expect(classifyWeapon(smallArm)).toBe('INFANTRY');
		expect(countsAsInfantry(smallArm)).toBe(true);
		expect(countsAsInfantry({ ...smallArm, victimFaction: null })).toBe(false);
		expect(countsAsInfantry({ ...smallArm, victimFaction: 'Blue' })).toBe(false);
	});

	test('old session-derived faction labels do not become verified infantry evidence', () => {
		expect(countsAsInfantry({ ...smallArm, factionBracketed: true })).toBe(false);
		expect(
			countsAsInfantry({
				...smallArm,
				factionBracketed: true,
				factionObservedAt: '2026-09-26T00:00:00.000Z'
			})
		).toBe(true);
	});

	test('unmapped item stays UNKNOWN and cannot inflate infantry KPM', () => {
		const unknown = { ...smallArm, cause: 'Id.Item.NotObserved' };
		expect(classifyWeapon(unknown)).toBe('UNKNOWN');
		expect(countsAsInfantry(unknown)).toBe(false);
	});

	test('context tags outrank even an admin infantry mapping', () => {
		const overrides = new Map([['Id.Item.NotObserved', 'INFANTRY' as const]]);
		const roadkill = { ...smallArm, cause: 'Id.Item.NotObserved', tags: ['RoadKill'] };
		expect(classifyWeapon(roadkill, overrides)).toBe('ROADKILL');
		expect(countsAsInfantry(roadkill, overrides)).toBe(false);
		expect(classifyWeapon({ ...smallArm, tags: ['VehicleExplosion'] })).toBe('VEHICLE');
	});

	test('explicit mortar and artillery mappings never count as infantry', () => {
		for (const category of ['MORTAR', 'ARTILLERY', 'FIXED_AA', 'CIWS'] as const) {
			const overrides = new Map([[smallArm.cause, category]]);
			expect(classifyWeapon(smallArm, overrides)).toBe(category);
			expect(countsAsInfantry(smallArm, overrides)).toBe(false);
		}
	});
});
