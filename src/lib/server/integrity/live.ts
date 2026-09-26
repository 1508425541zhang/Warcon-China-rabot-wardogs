import type { KillRow } from '../db/schema';
import type { Status } from '$lib/types';
import { classifyWeapon, countsAsInfantry, type WeaponCategory } from './weapons';

export interface LiveInfantryMetrics {
	infantryKills180: number;
	kpm180: number;
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
	if (!newest || (status?.map && status.map !== newest.map)) return new Map();
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
	const clock = Math.max(latestClock, status?.matchSeconds ?? latestClock);
	const byPlayer = new Map<string, KillRow[]>();
	const uncertain = new Set<string>();
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
				uncertain.add(row.killerSteamId);
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
	for (const steamId of new Set([...byPlayer.keys(), ...uncertain])) {
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
		result.set(steamId, {
			infantryKills180: current.length,
			kpm180: current.length / 3,
			peakKpm180: peak / 3,
			uniqueVictims180: new Set(current.map((row) => row.victimSteamId)).size,
			reliable: !uncertain.has(steamId)
		});
	}
	return result;
}
