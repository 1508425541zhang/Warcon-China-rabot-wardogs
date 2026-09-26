/** Server-feed cause classification. Exact mappings are reviewable; unknown stays unknown. */
export const WEAPON_CATEGORIES = [
	'INFANTRY',
	'VEHICLE',
	'MORTAR',
	'ARTILLERY',
	'FIXED_AA',
	'CIWS',
	'FIXED_WEAPON',
	'ROADKILL',
	'ENVIRONMENT',
	'SUICIDE',
	'UNKNOWN'
] as const;

export type WeaponCategory = (typeof WEAPON_CATEGORIES)[number];

/** Only small arms explicitly named in Warcon's observed cause catalogue. */
export const DEFAULT_WEAPON_MAP: Readonly<Record<string, WeaponCategory>> = {
	'Id.Item.AK74M': 'INFANTRY',
	'Id.Item.WEPN_029': 'INFANTRY',
	'Id.Item.M4': 'INFANTRY',
	'Id.Item.M500': 'INFANTRY',
	'Id.Item.MP43': 'INFANTRY',
	'Id.Item.SKS': 'INFANTRY',
	'Id.Item.SVDM': 'INFANTRY',
	'Id.Item.KH2002': 'INFANTRY',
	'Id.Item.TAR21': 'INFANTRY',
	'Id.Item.A91': 'INFANTRY',
	'Id.Item.SV98': 'INFANTRY',
	'Id.Item.MK22': 'INFANTRY',
	'Id.Item.Glock17': 'INFANTRY',
	'Id.Item.CombatBow': 'INFANTRY'
};

export function isWeaponCategory(value: unknown): value is WeaponCategory {
	return typeof value === 'string' && WEAPON_CATEGORIES.includes(value as WeaponCategory);
}

/** Tags that describe the kill itself outrank any weapon name or admin cause mapping. */
export function classifyWeapon(
	kill: { cause: string | null; tags: readonly string[]; suicide: boolean },
	overrides: ReadonlyMap<string, WeaponCategory> = new Map()
): WeaponCategory {
	if (kill.suicide || kill.tags.includes('Suicide')) return 'SUICIDE';
	if (kill.tags.includes('Falling')) return 'ENVIRONMENT';
	if (kill.tags.includes('RoadKill')) return 'ROADKILL';
	if (kill.tags.includes('VehicleExplosion')) return 'VEHICLE';
	if (!kill.cause) return 'UNKNOWN';
	const explicit = overrides.get(kill.cause);
	if (explicit) return explicit;
	const known = DEFAULT_WEAPON_MAP[kill.cause];
	if (known) return known;
	if (/^(?:Vehicle\.|Id\.Vehicle\.WeaponExtension\.)/i.test(kill.cause)) return 'VEHICLE';
	if (/^Id\.Buildable\./i.test(kill.cause)) return 'FIXED_WEAPON';
	return 'UNKNOWN';
}

/** An absent faction is ambiguous because the feed has no native faction fields. */
export function countsAsInfantry(
	kill: {
		cause: string | null;
		tags: readonly string[];
		suicide: boolean;
		teamKill?: boolean;
		killerSteamId: string | null;
		victimSteamId: string;
		killerFaction: string | null;
		victimFaction: string | null;
		factionBracketed?: boolean;
		factionObservedAt?: Date | string | null;
	},
	overrides: ReadonlyMap<string, WeaponCategory> = new Map()
): boolean {
	return (
		!!kill.killerSteamId &&
		kill.factionBracketed !== false &&
		(kill.factionBracketed !== true || !!kill.factionObservedAt) &&
		!kill.teamKill &&
		kill.killerSteamId !== kill.victimSteamId &&
		!!kill.killerFaction &&
		!!kill.victimFaction &&
		kill.killerFaction !== kill.victimFaction &&
		classifyWeapon(kill, overrides) === 'INFANTRY'
	);
}
