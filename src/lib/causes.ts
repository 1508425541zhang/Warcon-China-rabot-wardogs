// Labels for the cause tags the kill feed sends (`Id.Item.AK74M`, `Vehicle.Variant.Air.Rotary.
// Littlebird.Default`, ...). The game sends no display names, so the ones seen so far are named
// here and anything else falls back to a readable form of its last segments. Client-safe.

export type CauseKind = 'weapon' | 'vehicle weapon' | 'vehicle' | 'buildable' | 'none';

const LABELS: Record<string, string> = {
	'Id.Item.AK74M': 'AK-74M',
	'Id.Item.WEPN_029': 'Galil',
	'Id.Item.M4': 'M4',
	'Id.Item.M500': 'M500',
	'Id.Item.MP43': 'MP43',
	'Id.Item.SKS': 'SKS',
	'Id.Item.SVDM': 'SVDM',
	'Id.Item.KH2002': 'KH2002',
	'Id.Item.TAR21': 'TAR-21',
	'Id.Item.A91': 'A-91',
	'Id.Item.SV98': 'SV-98',
	'Id.Item.RPG7': 'RPG-7',
	'Id.Item.MK22': 'MK 22',
	'Id.Item.M67Grenade': 'M67 grenade',
	'Id.Item.C4Explosive': 'C4',
	'Id.Item.Glock17': 'Glock 17',
	'Id.Item.CombatBow': 'Combat bow',
	'Id.Item.Defibrillator.Standard': 'Defibrillator',
	'ID.Item.BuildTool.Hammer.Large': 'Hammer (large)',
	'ID.Item.BuildTool.Hammer.Medium': 'Hammer (medium)',
	'Id.Buildable.BremmerWall': 'Bremer wall',
	'Id.Buildable.BarbedWire': 'Barbed wire',
	'Id.Vehicle.WeaponExtension.STN_01.MistralAA': 'STN 01 Mistral AA',
	'Id.Vehicle.WeaponExtension.STN_02.MainCannon': 'STN 02 main cannon',
	'Id.Vehicle.WeaponExtension.STN_03.MainBarrel': 'STN 03 main gun',
	'Id.Vehicle.WeaponExtension.ROT_02.30mmCannon': 'ROT 02 30 mm cannon',
	'Id.Vehicle.WeaponExtension.ROT_02.122mm': 'ROT 02 122 mm',
	'Id.Vehicle.WeaponExtension.ROT_03.MountedMachineGun': 'ROT 03 mounted MG',
	'Id.Vehicle.WeaponExtension.WHL_02.SUV.RingTurret': 'SUV ring turret',
	'Vehicle.Variant.Air.Rotary.Littlebird.MountedMachineGuns': 'Littlebird (mounted MGs)',
	'Vehicle.Variant.Land.Wheeled.Kodiak.MachineGun': 'Kodiak (machine gun)',
	'Vehicle.Variant.Land.Wheeled.Kodiak.Pickup': 'Kodiak pickup',
	'Vehicle.Variant.Land.Tracked.SpawnVehicle.Lonestar': 'Lonestar'
};

/** What sort of thing the cause is, from its prefix. */
export function causeKind(cause: string | null | undefined): CauseKind {
	if (!cause) return 'none';
	if (/^Id\.Vehicle\.WeaponExtension\./i.test(cause)) return 'vehicle weapon';
	if (/^Vehicle\./i.test(cause)) return 'vehicle';
	if (/^Id\.Buildable\./i.test(cause)) return 'buildable';
	return 'weapon';
}

/** `WEPN_035` → `WEPN 035`, `MountedMachineGuns` → `Mounted machine guns`: a codename keeps its capitals. */
function pretty(segment: string): string {
	const words = segment
		.replace(/_/g, ' ')
		.replace(/([a-z])([A-Z])/g, '$1 $2')
		.replace(/([A-Za-z])(\d)/g, '$1 $2')
		.trim()
		.split(/\s+/);
	return words
		.map((w, i) => {
			if (/^[A-Z0-9]+$/.test(w) && /[A-Z]{2}/.test(w)) return w;
			const lower = w.toLowerCase();
			return i === 0 ? lower.charAt(0).toUpperCase() + lower.slice(1) : lower;
		})
		.join(' ');
}

/** Every cause named above, for a filter's choices: label first so a list reads alphabetically. */
export function knownCauses(): { cause: string; label: string }[] {
	return Object.entries(LABELS)
		.map(([cause, label]) => ({ cause, label }))
		.sort((a, b) => a.label.localeCompare(b.label));
}

/** A display name for the tag: the known ones by name, the rest from their meaningful segments. */
export function causeLabel(cause: string | null | undefined): string {
	if (!cause) return '';
	const known = LABELS[cause];
	if (known) return known;
	const segs = cause.split('.').filter(Boolean);
	switch (causeKind(cause)) {
		case 'vehicle': {
			// Vehicle.Variant.Air.Rotary.Littlebird.Default: the model, plus the variant unless Default.
			const [model, variant] = segs.slice(4);
			return variant && variant !== 'Default'
				? `${pretty(model ?? '')} (${pretty(variant).toLowerCase()})`
				: pretty(model ?? segs[segs.length - 1]);
		}
		case 'vehicle weapon':
			// Id.Vehicle.WeaponExtension.STN_03.MainBarrel: the mount and the weapon.
			return segs.slice(3).map(pretty).join(' ');
		case 'buildable':
			return pretty(segs[segs.length - 1]);
		default:
			// Id.Item.Mosin, Id.Item.Defibrillator.Standard
			return segs.slice(2).map(pretty).join(' ');
	}
}
