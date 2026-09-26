import type { KillRow } from '../db/schema';
import { mapId } from '$lib/format';
import type { Status } from '$lib/types';
import { classifyWeapon, countsAsInfantry, type WeaponCategory } from './weapons';

export interface LiveInfantryMetrics {
	infantryKills180: number;
	kpm180: number;
	infantryKills60: number;
	kpm60: number;
	reliable180: boolean;
	reliable60: boolean;
	peakKpm180: number;
	uniqueVictims180: number;
	/** A plausible infantry kill lacked a reliable weapon or faction classification. */
	reliable: boolean;
}

/** Reconstruct current-match infantry figures from accepted feed rows. No KD risk is inferred. */
export function liveInfantryMetrics(
	rows: readonly KillRow[],
	status: Pick<Status, 'map' | 'matchSeconds'> | null,
	overrides: ReadonlyMap<string, WeaponCategory>
): Map<string, LiveInfantryMetrics> {
	const newest = rows[0];
	if (!newest || (status?.map && mapId(status.map) !== mapId(newest.map))) return new Map();
	const matchRows = rows.filter(
		(row) => row.instanceId === newest.instanceId && row.map === newest.map
	);
	const latestClock = Math.max(...matchRows.map((row) => row.eventTime));
	if (
		status?.matchSeconds !== null &&
		status?.matchSeconds !== undefined &&
		status.matchSeconds < latestClock - 5
	)
		return new Map();
	// Some live builds omit the game clock. Keep windows advancing between feed arrivals.
	const elapsed =
		newest.ts instanceof Date ? Math.max(0, (Date.now() - newest.ts.getTime()) / 1000) : 0;
	const clock = Math.max(latestClock, status?.matchSeconds ?? latestClock + elapsed);
	const byPlayer = new Map<string, KillRow[]>();
	const uncertain = new Map<string, number>();
	for (const row of matchRows) {
		if (
			row.killerSteamId &&
			row.killerSteamId !== row.victimSteamId &&
			!row.suicide &&
			row.eventTime > clock - 600 &&
			row.eventTime <= clock
		) {
			const category = classifyWeapon(
				{
					cause: row.cause,
					tags: Array.isArray(row.tags) ? (row.tags as string[]) : [],
					suicide: row.suicide
				},
				overrides
			);
			if (
				category === 'UNKNOWN' ||
				(category === 'INFANTRY' &&
					(!row.killerFaction ||
						!row.victimFaction ||
						row.factionBracketed === false ||
						(row.factionBracketed && !row.factionObservedAt)))
			)
				uncertain.set(
					row.killerSteamId,
					Math.max(uncertain.get(row.killerSteamId) ?? -Infinity, row.eventTime)
				);
		}
		if (
			!countsAsInfantry(
				{
					cause: row.cause,
					tags: Array.isArray(row.tags) ? (row.tags as string[]) : [],
					suicide: row.suicide,
					teamKill: row.teamKill,
					killerSteamId: row.killerSteamId,
					victimSteamId: row.victimSteamId,
					killerFaction: row.killerFaction,
					victimFaction: row.victimFaction,
					factionBracketed: row.factionBracketed,
					factionObservedAt: row.factionObservedAt
				},
				overrides
			)
		)
			continue;
		const player = byPlayer.get(row.killerSteamId!) ?? [];
		player.push(row);
		byPlayer.set(row.killerSteamId!, player);
	}
	const result = new Map<string, LiveInfantryMetrics>();
	for (const steamId of new Set([...byPlayer.keys(), ...uncertain.keys()])) {
		const playerRows = byPlayer.get(steamId) ?? [];
		playerRows.sort((a, b) => a.eventTime - b.eventTime);
		let start = 0;
		let peak = 0;
		for (let end = 0; end < playerRows.length; end++) {
			while (playerRows[start].eventTime <= playerRows[end].eventTime - 180) start++;
			peak = Math.max(peak, end - start + 1);
		}
		const current = playerRows.filter(
			(row) => row.eventTime > clock - 180 && row.eventTime <= clock
		);
		const short = current.filter((row) => row.eventTime > clock - 60);
		const unknownClock = uncertain.get(steamId) ?? -Infinity;
		result.set(steamId, {
			infantryKills180: current.length,
			kpm180: current.length / 3,
			infantryKills60: short.length,
			kpm60: short.length,
			reliable180: unknownClock <= clock - 180,
			reliable60: unknownClock <= clock - 60,
			peakKpm180: peak / 3,
			uniqueVictims180: new Set(current.map((row) => row.victimSteamId)).size,
			reliable: !uncertain.has(steamId)
		});
	}
	return result;
}
