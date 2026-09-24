// The reserved-slot list as it lives in ServerSettings.ini, for builds without the live
// reserved-slot routes (live builds CL-499480 and CL-501228): the same SteamIDs GET /v1/reserved-slots
// reports, read from and written to the DefaultReservedPlayerIds array of the config document's
// [/Script/WDGame.WDGameSession] section, the way the official console does it.
import { getArray, getScalar, parseIni, setArrayInText } from './config-doc';
import { S_SESSION } from './config-fields';

export const RESERVED_KEY = 'DefaultReservedPlayerIds';
export const HELD_KEY = 'MaxReservedSlots';

/**
 * MaxReservedSlots: how many of MaxPlayers the server holds back from public joins for players on
 * the reserved list (the live server reports MaxPlayers minus this as its player cap). It does not
 * limit the list itself, which the server takes at any length. Null when the document does not
 * set it.
 */
export function reservedSlotsHeld(text: string): number | null {
	const raw = getScalar(parseIni(text), S_SESSION, HELD_KEY);
	if (raw === null) return null;
	const n = Number(raw.trim());
	return Number.isInteger(n) && n >= 0 ? n : null;
}

/** The SteamIDs the document reserves, in file order, quotes stripped. */
export function reservedFromText(text: string): string[] {
	return getArray(parseIni(text), S_SESSION, RESERVED_KEY)
		.map((v) => v.trim())
		.filter(Boolean);
}

/** Writes the list back as `!Key=ClearArray` + one `.Key=` line per id, touching nothing else. */
export function reservedIntoText(text: string, ids: string[]): string {
	return setArrayInText(text, S_SESSION, RESERVED_KEY, ids);
}

/** Whether the document sets the key at all (in any array command form). */
export const hasReservedKey = (text: string): boolean =>
	new RegExp(`^\\s*[+.!-]?${RESERVED_KEY}\\s*=`, 'im').test(text);
