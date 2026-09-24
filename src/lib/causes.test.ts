import { describe, expect, test } from 'bun:test';
import { causeKind, causeLabel, knownCauses } from './causes';

describe('causeLabel', () => {
	test('named weapons and vehicles', () => {
		expect(causeLabel('Id.Item.AK74M')).toBe('AK-74M');
		expect(causeLabel('Id.Item.WEPN_029')).toBe('Galil');
		expect(causeLabel('Id.Item.M4')).toBe('M4');
		expect(causeLabel('Id.Item.M67Grenade')).toBe('M67 grenade');
		expect(causeLabel('Vehicle.Variant.Land.Tracked.SpawnVehicle.Lonestar')).toBe('Lonestar');
	});

	test('unnamed tags read from their segments', () => {
		expect(causeLabel('Id.Item.Mosin')).toBe('Mosin');
		expect(causeLabel('Id.Item.WEPN_035')).toBe('WEPN 035');
		expect(causeLabel('Id.Item.SMG_03')).toBe('SMG 03');
		expect(causeLabel('Vehicle.Variant.Air.Rotary.Littlebird.Default')).toBe('Littlebird');
		expect(causeLabel('Vehicle.Variant.Air.Rotary.ROT_04.Default')).toBe('ROT 04');
		expect(causeLabel('Vehicle.Variant.Land.Wheeled.Ural.Default')).toBe('Ural');
		expect(causeLabel('Id.Vehicle.WeaponExtension.STN_09.Turret')).toBe('STN 09 Turret');
		expect(causeLabel('Id.Buildable.Gate')).toBe('Gate');
		expect(causeLabel('Id.Vehicle.WeaponExtension.WHL_05.RingTurret')).toBe('WHL 05 Ring turret');
	});

	test('the named causes as filter choices, alphabetical by label', () => {
		const list = knownCauses();
		expect(list.find((c) => c.cause === 'Id.Item.WEPN_029')?.label).toBe('Galil');
		expect(list.map((c) => c.label)).toEqual(
			[...list.map((c) => c.label)].sort((a, b) => a.localeCompare(b))
		);
	});

	test('nothing for no cause', () => {
		expect(causeLabel(null)).toBe('');
		expect(causeLabel('')).toBe('');
	});
});

describe('causeKind', () => {
	test('by prefix', () => {
		expect(causeKind('Id.Item.AK74M')).toBe('weapon');
		expect(causeKind('ID.Item.BuildTool.Hammer.Large')).toBe('weapon');
		expect(causeKind('Id.Vehicle.WeaponExtension.STN_03.MainBarrel')).toBe('vehicle weapon');
		expect(causeKind('Vehicle.Variant.Air.Rotary.Littlebird.Default')).toBe('vehicle');
		expect(causeKind('Id.Buildable.Gate')).toBe('buildable');
		expect(causeKind(null)).toBe('none');
	});
});
