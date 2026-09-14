// The reserved-slot list as it lives in ServerSettings.ini, for builds without the live
// reserved-slot routes (live builds CL-499480 and CL-501228): the same SteamIDs GET /v1/reserved-slots
// reports, read from and written to the DefaultReservedPlayerIds array of the config document's
// [/Script/WDGame.WDGameSession] section, the way the official console does it.
import { getArray, parseIni, setArrayInText } from './config-doc';
import { S_SESSION } from './config-fields';

export const RESERVED_KEY = 'DefaultReservedPlayerIds';

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
