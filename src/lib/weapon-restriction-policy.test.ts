import { expect, test } from 'bun:test';
import { restrictedCause, restrictionStage } from './weapon-restriction-policy';
test('all source families and newly discovered exact IDs are selectable', () => {
	for (const c of ['Id.Item.M4', 'Id.Item.M67Grenade', 'ID.Item.FutureMine'])
		expect(restrictedCause(c, [], ['items'])).toBe(true);
	for (const c of ['Vehicle.Variant.Air.Test', 'Id.Vehicle.WeaponExtension.Unknown'])
		expect(restrictedCause(c, [], ['vehicles'])).toBe(true);
	expect(restrictedCause('Id.Buildable.Turret', [], ['buildables'])).toBe(true);
	expect(restrictedCause('Future.SpecialWeapon', ['Future.SpecialWeapon'], [])).toBe(true);
	expect(restrictedCause(null, [], ['items'])).toBe(false);
	expect(restrictedCause('Id.Item.M4', [], ['vehicles'])).toBe(false);
});
test('warning must precede another kill; grace, stale feed and kick cooldown', () => {
	expect(restrictionStage(100, 100, undefined, null)).toBe('warn');
	expect(restrictionStage(100, 110, { clock: 100 }, null)).toBeNull();
	expect(restrictionStage(108, 110, { clock: 100 }, null)).toBe('kick');
	expect(restrictionStage(40, 110, undefined, null)).toBeNull();
	expect(restrictionStage(116, 110, undefined, null)).toBeNull();
	expect(restrictionStage(120, 120, { clock: 100 }, 110)).toBeNull();
	expect(restrictionStage(170, 170, { clock: 100 }, 110)).toBe('kick');
});
