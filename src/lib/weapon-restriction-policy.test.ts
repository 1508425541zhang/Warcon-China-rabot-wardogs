import { expect, test } from 'bun:test';
import { restrictedCause, restrictionStage, restrictionClock } from './weapon-restriction-policy';
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
test('missing live clock uses fresh feed, never an old or invalid anchor', () => {
	expect(restrictionClock(null, 100000, 100000, { eventClock: 600, receivedAt: 99000 })).toBe(601);
	expect(restrictionClock(500, 99000, 100000, null)).toBe(501);
	expect(restrictionClock(null, 100000, 100000, null)).toBeNull();
	expect(restrictionClock(null, 100000, 100000, { eventClock: 600, receivedAt: 69000 })).toBeNull();
	expect(
		restrictionClock(null, 100000, 100000, { eventClock: 600, receivedAt: 106000 })
	).toBeNull();
	expect(restrictionClock(null, 60000, 100000, { eventClock: 600, receivedAt: 99000 })).toBeNull();
	expect(restrictionClock(null, 100000, 100000, { eventClock: NaN, receivedAt: 99000 })).toBeNull();
});
test('first restricted kill kicks directly; stale feed and kick cooldown', () => {
	expect(restrictionStage(100, 100, null)).toBe('kick');
	expect(restrictionStage(100, 110, null)).toBe('kick');
	expect(restrictionStage(40, 110, null)).toBeNull();
	expect(restrictionStage(116, 110, null)).toBeNull();
	expect(restrictionStage(NaN, 110, null)).toBeNull();
	expect(restrictionStage(120, 120, 110)).toBeNull();
	expect(restrictionStage(170, 170, 110)).toBe('kick');
});
