import type { KillView } from '$lib/types';
import { countsAsInfantry, type WeaponCategory } from './weapons';

export const INFANTRY_WINDOW_SECONDS = 180;
export const DEFAULT_ABNORMAL_KPM = 4;

interface Entry {
	clock: number;
	eventId: string;
	victimSteamId: string;
}

interface PlayerWindow {
	entries: Entry[];
	lastFindingClock: number | null;
	peakKpm: number;
}

interface ServerWindow {
	instanceId: string;
	map: string;
	latestClock: number;
	players: Map<string, PlayerWindow>;
}

export interface InfantryFinding {
	steamId: string;
	instanceId: string;
	map: string;
	clockFrom: number;
	clockTo: number;
	infantryKills: number;
	kpm180: number;
	uniqueVictims: number;
	eventIds: string[];
}

/** One worker's live windows. Persisted findings are advisory; restart starts a fresh window. */
export class InfantryWindows {
	private servers = new Map<string, ServerWindow>();

	reset(serverId?: string): void {
		if (serverId) this.servers.delete(serverId);
		else this.servers.clear();
	}

	observe(
		serverId: string,
		batch: readonly KillView[],
		overrides: ReadonlyMap<string, WeaponCategory>,
		abnormalKpm = DEFAULT_ABNORMAL_KPM
	): InfantryFinding[] {
		const findings: InfantryFinding[] = [];
		for (const kill of batch) {
			const instanceId = kill.instanceId;
			const clock = kill.eventTime;
			if (!instanceId || !kill.map || !Number.isFinite(clock) || clock < 0) {
				this.reset(serverId);
				continue;
			}
			let server = this.servers.get(serverId);
			// A new map or per-boot instance starts a new match. A large clock rewind on the same
			// map could be a new round or delayed feed: reset conservatively in either case.
			if (
				!server ||
				server.instanceId !== instanceId ||
				server.map !== kill.map ||
				clock < server.latestClock - 5
			) {
				server = { instanceId, map: kill.map, latestClock: clock, players: new Map() };
				this.servers.set(serverId, server);
			}
			server.latestClock = Math.max(server.latestClock, clock);
			const killerSteamId = kill.killer?.steamId ?? null;
			if (!killerSteamId) continue;
			if (
				!countsAsInfantry(
					{
						cause: kill.cause,
						tags: kill.tags,
						suicide: kill.suicide,
						killerSteamId,
						victimSteamId: kill.victim.steamId,
						killerFaction: kill.killer?.faction ?? null,
						victimFaction: kill.victim.faction
					},
					overrides
				)
			)
				continue;
			if (clock <= server.latestClock - INFANTRY_WINDOW_SECONDS) continue;
			let player = server.players.get(killerSteamId);
			if (!player) {
				player = { entries: [], lastFindingClock: null, peakKpm: 0 };
				server.players.set(killerSteamId, player);
			}
			if (player.entries.some((e) => e.eventId === kill.eventId)) continue;
			const entry = { clock, eventId: kill.eventId, victimSteamId: kill.victim.steamId };
			let at = player.entries.length;
			while (at > 0 && player.entries[at - 1].clock > clock) at--;
			player.entries.splice(at, 0, entry);
			const from = server.latestClock - INFANTRY_WINDOW_SECONDS;
			while (player.entries.length && player.entries[0].clock <= from) player.entries.shift();
			const kpm180 = player.entries.length / 3;
			player.peakKpm = Math.max(player.peakKpm, kpm180);
			if (kpm180 < abnormalKpm) continue;
			if (
				player.lastFindingClock !== null &&
				server.latestClock - player.lastFindingClock < INFANTRY_WINDOW_SECONDS
			)
				continue;
			player.lastFindingClock = server.latestClock;
			findings.push({
				steamId: killerSteamId,
				instanceId,
				map: kill.map,
				clockFrom: player.entries[0].clock,
				clockTo: server.latestClock,
				infantryKills: player.entries.length,
				kpm180,
				uniqueVictims: new Set(player.entries.map((e) => e.victimSteamId)).size,
				eventIds: player.entries.map((e) => e.eventId)
			});
		}
		const server = this.servers.get(serverId);
		if (server) {
			const from = server.latestClock - INFANTRY_WINDOW_SECONDS;
			for (const [steamId, player] of server.players) {
				while (player.entries.length && player.entries[0].clock <= from) player.entries.shift();
				if (
					!player.entries.length &&
					(player.lastFindingClock === null || server.latestClock - player.lastFindingClock > 900)
				)
					server.players.delete(steamId);
			}
		}
		return findings;
	}

	current(serverId: string, steamId: string): { kpm180: number; peakKpm180: number } | null {
		const server = this.servers.get(serverId);
		const player = server?.players.get(steamId);
		return player && server
			? {
					kpm180:
						player.entries.filter(
							(entry) => entry.clock > server.latestClock - INFANTRY_WINDOW_SECONDS
						).length / 3,
					peakKpm180: player.peakKpm
				}
			: null;
	}
}
