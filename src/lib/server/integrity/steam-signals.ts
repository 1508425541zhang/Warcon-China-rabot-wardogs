import { STEAM_MAX_AGE_MS, type SteamProfileRow } from '../steam';

export function steamBanSignals(
	profile:
		| Pick<SteamProfileRow, 'fetchedAt' | 'error' | 'vacBans' | 'gameBans' | 'daysSinceLastBan'>
		| undefined,
	now: Date
): { known: boolean; vacBans: number; gameBans: number; daysSinceLastBan: number | null } {
	const known =
		!!profile &&
		!profile.error &&
		profile.fetchedAt.getTime() <= now.getTime() &&
		now.getTime() - profile.fetchedAt.getTime() <= STEAM_MAX_AGE_MS;
	return {
		known,
		vacBans: known ? profile.vacBans : 0,
		gameBans: known ? profile.gameBans : 0,
		daysSinceLastBan: known ? profile.daysSinceLastBan : null
	};
}
