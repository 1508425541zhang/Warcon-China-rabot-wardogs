import type { KillView } from '$lib/types';
import { validKillDistanceM } from '../feed-core';
import { countsAsInfantry, type WeaponCategory } from './weapons';
import { DEFAULT_INTEGRITY_RULES, type BehaviorReason, type IntegrityRuleConfig } from './score';

export const INFANTRY_WINDOW_SECONDS = 180;

interface Entry {
	clock: number;
	eventId: string;
	victimSteamId: string;
	cause: string | null;
	distanceM: number | null;
	headshot: boolean;
	penetration: boolean;
}

interface PlayerWindow {
	entries: Entry[];
	/** All valid event IDs connected to the current abnormal episode. */
	episodeEventIds: Set<string>;
	lastFindingClock: number | null;
	windowId: number | null;
	best: {
		kpm: number;
		victims: number;
		headshot: boolean;
		penetration: boolean;
		burst: number;
	} | null;
	lastReasons: BehaviorReason[];
	peakKpm: number;
}

interface ServerWindow {
	instanceId: string;
	matchRow: number | null;
	roundSequence: number;
	map: string;
	latestClock: number;
	players: Map<string, PlayerWindow>;
}

/** Uses game event clocks only. Each burst tier is exclusive; the best matching tier wins. */
export function burstPoints(entries: readonly Pick<Entry, 'clock'>[]): number {
	const clocks = entries.map((entry) => entry.clock).sort((a, b) => a - b);
	for (const [count, seconds, points] of [
		[8, 15, 12],
		[6, 12, 10],
		[5, 10, 7],
		[4, 8, 4],
		[3, 5, 2]
	] as const) {
		for (let i = 0; i + count <= clocks.length; i++)
			if (clocks[i + count - 1] - clocks[i] <= seconds) return points;
	}
	return 0;
}

export function maxKillsWithin(entries: readonly Pick<Entry, 'clock'>[], seconds: number): number {
	const clocks = entries.map((entry) => entry.clock).sort((a, b) => a - b);
	let start = 0;
	let maximum = 0;
	for (let end = 0; end < clocks.length; end++) {
		while (clocks[start] < clocks[end] - seconds) start++;
		maximum = Math.max(maximum, end - start + 1);
	}
	return maximum;
}

export function medianKillInterval(entries: readonly Pick<Entry, 'clock'>[]): number | null {
	if (entries.length < 2) return null;
	const clocks = entries.map((entry) => entry.clock).sort((a, b) => a - b);
	const gaps = clocks
		.slice(1)
		.map((clock, i) => clock - clocks[i])
		.sort((a, b) => a - b);
	const middle = Math.floor(gaps.length / 2);
	return gaps.length % 2 ? gaps[middle] : (gaps[middle - 1] + gaps[middle]) / 2;
}

export interface WeaponWindowMetric {
	cause: string;
	kills: number;
	headshots: number;
	maxKillDistanceM: number | null;
}

export function weaponWindowMetrics(entries: readonly Entry[]): WeaponWindowMetric[] {
	const byCause = new Map<string, WeaponWindowMetric>();
	for (const entry of entries) {
		if (!entry.cause) continue;
		const item = byCause.get(entry.cause) ?? {
			cause: entry.cause,
			kills: 0,
			headshots: 0,
			maxKillDistanceM: null
		};
		item.kills++;
		if (entry.headshot) item.headshots++;
		if (entry.distanceM !== null && validKillDistanceM(entry.distanceM) && entry.distanceM > 0)
			item.maxKillDistanceM = Math.max(item.maxKillDistanceM ?? 0, entry.distanceM);
		byCause.set(entry.cause, item);
	}
	return [...byCause.values()];
}

export interface BehaviorFinding {
	steamId: string;
	instanceId: string;
	roundId: string;
	map: string;
	/** The first finding's game clock identifies one active window across upgrades. */
	anchorClock: number;
	windowId: number | null;
	clockFrom: number;
	clockTo: number;
	infantryKills: number;
	kpm180: number;
	uniqueVictims: number;
	headshots: number;
	headshotPct: number;
	penetrations: number;
	penetrationPct: number;
	burstPoints: number;
	maxKills15s: number;
	medianKillInterval: number | null;
	weaponMetrics?: WeaponWindowMetric[];
	reasons: BehaviorReason[];
	/** True when no legacy severity tier changed on this exact event. */
	snapshotOnly?: boolean;
	eventIds: string[];
}

const tier = (value: number, bands: readonly { min: number; points: number }[]) =>
	bands.reduce((result, band, index) => (value >= band.min ? index + 1 : result), 0);

/** One worker's live windows. A restart conservatively starts fresh. */
export class InfantryWindows {
	private servers = new Map<string, ServerWindow>();

	reset(serverId?: string): void {
		if (serverId) this.servers.delete(serverId);
		else this.servers.clear();
	}
	hasServer(serverId: string): boolean {
		return this.servers.has(serverId);
	}

	markPersisted(serverId: string, finding: BehaviorFinding, windowId: number): void {
		const player = this.servers.get(serverId)?.players.get(finding.steamId);
		if (player?.lastFindingClock === finding.anchorClock) player.windowId = windowId;
	}

	/** Current accepted infantry evidence, including behavior below the legacy finding thresholds. */
	snapshots(serverId: string, steamIds: readonly string[]): BehaviorFinding[] {
		const server = this.servers.get(serverId);
		if (!server) return [];
		const result: BehaviorFinding[] = [];
		for (const steamId of new Set(steamIds)) {
			const player = server.players.get(steamId);
			if (!player?.entries.length) continue;
			const entries = player.entries.filter(
				(entry) => entry.clock > server.latestClock - INFANTRY_WINDOW_SECONDS
			);
			if (!entries.length) continue;
			const eventIds = entries.map((entry) => entry.eventId);
			const active = eventIds.some((eventId) => player.episodeEventIds.has(eventId));
			const headshots = entries.filter((entry) => entry.headshot).length;
			const penetrations = entries.filter((entry) => entry.penetration).length;
			result.push({
				steamId,
				instanceId: server.instanceId,
				roundId:
					server.matchRow === null
						? `${server.instanceId}:derived:${server.roundSequence}`
						: `${server.instanceId}:match:${server.matchRow}`,
				map: server.map,
				anchorClock: active ? player.lastFindingClock! : server.latestClock,
				windowId: active ? player.windowId : null,
				clockFrom: entries[0].clock,
				clockTo: server.latestClock,
				infantryKills: entries.length,
				kpm180: entries.length / 3,
				uniqueVictims: new Set(entries.map((entry) => entry.victimSteamId)).size,
				headshots,
				headshotPct: (100 * headshots) / entries.length,
				penetrations,
				penetrationPct: (100 * penetrations) / entries.length,
				burstPoints: burstPoints(entries),
				maxKills15s: maxKillsWithin(entries, 15),
				medianKillInterval: medianKillInterval(entries),
				weaponMetrics: weaponWindowMetrics(entries),
				reasons: active ? player.lastReasons : [],
				snapshotOnly: true,
				eventIds
			});
		}
		return result;
	}

	observe(
		serverId: string,
		batch: readonly KillView[],
		overrides: ReadonlyMap<string, WeaponCategory>,
		config: IntegrityRuleConfig = DEFAULT_INTEGRITY_RULES
	): BehaviorFinding[] {
		const findings: BehaviorFinding[] = [];
		// Sort only a single known round. Sorting across maps by reset clocks reverses rounds.
		const oneRound = batch.every(
			(kill) =>
				kill.instanceId === batch[0]?.instanceId &&
				kill.map === batch[0]?.map &&
				kill.matchRow === batch[0]?.matchRow
		);
		for (const kill of oneRound ? [...batch].sort((a, b) => a.eventTime - b.eventTime) : batch) {
			const instanceId = kill.instanceId;
			const clock = kill.eventTime;
			if (!instanceId || !kill.map || !Number.isFinite(clock) || clock < 0) continue;
			let server = this.servers.get(serverId);
			const matchRow = kill.matchRow ?? null;
			if (
				server &&
				server.instanceId === instanceId &&
				server.matchRow !== null &&
				matchRow !== null &&
				matchRow < server.matchRow
			)
				continue;
			const explicitRoundChanged =
				!!server && server.matchRow !== null && matchRow !== null && matchRow > server.matchRow;
			const derivedRoundChanged =
				!!server &&
				(server.matchRow === null || matchRow === null) &&
				(server.map !== kill.map || (clock <= 5 && server.latestClock >= 20));
			if (
				!server ||
				server.instanceId !== instanceId ||
				explicitRoundChanged ||
				derivedRoundChanged
			) {
				server = {
					instanceId,
					matchRow,
					roundSequence: (server?.roundSequence ?? 0) + 1,
					map: kill.map,
					latestClock: clock,
					players: new Map()
				};
				this.servers.set(serverId, server);
			}
			// A delayed event from the same round cannot reset or contaminate a live window.
			if (server.map !== kill.map || clock <= server.latestClock - INFANTRY_WINDOW_SECONDS)
				continue;
			server.latestClock = Math.max(server.latestClock, clock);
			const killerSteamId = kill.killer?.steamId ?? null;
			if (!killerSteamId) continue;
			if (
				!countsAsInfantry(
					{
						cause: kill.cause,
						tags: kill.tags,
						suicide: kill.suicide,
						teamKill: kill.teamKill,
						killerSteamId,
						victimSteamId: kill.victim.steamId,
						killerFaction: kill.killer?.faction ?? null,
						victimFaction: kill.victim.faction,
						factionBracketed: kill.factionBracketed
					},
					overrides
				)
			)
				continue;
			if (clock <= server.latestClock - INFANTRY_WINDOW_SECONDS) continue;
			let player = server.players.get(killerSteamId);
			if (!player) {
				player = {
					entries: [],
					episodeEventIds: new Set(),
					lastFindingClock: null,
					windowId: null,
					best: null,
					lastReasons: [],
					peakKpm: 0
				};
				server.players.set(killerSteamId, player);
			}
			if (player.entries.some((entry) => entry.eventId === kill.eventId)) continue;
			const entry: Entry = {
				clock,
				eventId: kill.eventId,
				victimSteamId: kill.victim.steamId,
				cause: kill.cause,
				distanceM: kill.distanceM,
				headshot: kill.headshot,
				penetration: kill.tags.includes('Penetration')
			};
			let at = player.entries.length;
			while (at > 0 && player.entries[at - 1].clock > clock) at--;
			player.entries.splice(at, 0, entry);
			const from = server.latestClock - INFANTRY_WINDOW_SECONDS;
			while (player.entries.length && player.entries[0].clock <= from) player.entries.shift();
			const infantryKills = player.entries.length;
			const kpm180 = infantryKills / 3;
			player.peakKpm = Math.max(player.peakKpm, kpm180);
			const headshots = player.entries.filter((item) => item.headshot).length;
			const penetrations = player.entries.filter((item) => item.penetration).length;
			const headshotPct = (100 * headshots) / infantryKills;
			const penetrationPct = (100 * penetrations) / infantryKills;
			const burst = burstPoints(player.entries);
			const reasons: BehaviorReason[] = [];
			if (kpm180 >= config.kpmBands[0].min) reasons.push('kpm');
			if (infantryKills >= config.headshotMinKills && headshotPct >= config.headshotMinPct)
				reasons.push('headshot');
			if (infantryKills >= config.penetrationMinKills && penetrationPct >= config.penetrationMinPct)
				reasons.push('penetration');
			if (burst >= config.burstFindingMin) reasons.push('burst');
			if (!reasons.length) continue;
			const severity = {
				kpm: tier(kpm180, config.kpmBands),
				victims: reasons.includes('kpm')
					? tier(
							new Set(player.entries.map((item) => item.victimSteamId)).size,
							config.uniqueVictimBands
						)
					: 0,
				headshot: reasons.includes('headshot'),
				penetration: reasons.includes('penetration'),
				burst
			};
			const eventIds = player.entries.map((item) => item.eventId);
			const active =
				player.lastFindingClock !== null &&
				eventIds.some((eventId) => player.episodeEventIds.has(eventId));
			if (
				active &&
				player.best &&
				severity.kpm <= player.best.kpm &&
				severity.victims <= player.best.victims &&
				(!severity.headshot || player.best.headshot) &&
				(!severity.penetration || player.best.penetration) &&
				severity.burst <= player.best.burst
			)
				continue;
			if (!active) {
				player.lastFindingClock = server.latestClock;
				player.windowId = null;
				player.best = null;
				player.episodeEventIds = new Set(eventIds);
			}
			if (active) for (const eventId of eventIds) player.episodeEventIds.add(eventId);
			player.best = {
				kpm: Math.max(player.best?.kpm ?? 0, severity.kpm),
				victims: Math.max(player.best?.victims ?? 0, severity.victims),
				headshot: !!player.best?.headshot || severity.headshot,
				penetration: !!player.best?.penetration || severity.penetration,
				burst: Math.max(player.best?.burst ?? 0, severity.burst)
			};
			player.lastReasons = reasons;
			findings.push({
				steamId: killerSteamId,
				instanceId,
				roundId:
					server.matchRow === null
						? `${instanceId}:derived:${server.roundSequence}`
						: `${instanceId}:match:${server.matchRow}`,
				map: kill.map,
				anchorClock: player.lastFindingClock!,
				windowId: player.windowId,
				clockFrom: player.entries[0].clock,
				clockTo: server.latestClock,
				infantryKills,
				kpm180,
				uniqueVictims: new Set(player.entries.map((item) => item.victimSteamId)).size,
				headshots,
				headshotPct,
				penetrations,
				penetrationPct,
				burstPoints: burst,
				maxKills15s: maxKillsWithin(player.entries, 15),
				medianKillInterval: medianKillInterval(player.entries),
				weaponMetrics: weaponWindowMetrics(player.entries),
				reasons,
				eventIds
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
		const latest = new Map<string, BehaviorFinding>();
		for (const finding of findings)
			latest.set(`${finding.steamId}:${finding.instanceId}:${finding.anchorClock}`, finding);
		return [...latest.values()];
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
