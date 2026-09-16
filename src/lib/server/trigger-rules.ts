// The pure part of automation: trigger settings, their validation, message templates and the
// kick-on-connect verdict. No database, no game server, so it is unit-testable on its own;
// triggers.ts holds the engine that runs these against live ticks.
import { ApiError, int, str } from './http';
import { accountAgeDays } from './risk';
import { RESTART_AFTER_HOURS, restartWindow } from '$lib/uptime';
import type { SteamProfileRow } from './db/schema';
import type { TriggerKind } from '$lib/types';

export const TRIGGER_KINDS: TriggerKind[] = [
	'welcome',
	'faction_change',
	'broadcast',
	'empty_reset',
	'risk_kick',
	'restart_notice',
	'team_kill'
];
export const TRIGGER_LABELS: Record<TriggerKind, string> = {
	welcome: 'Welcome whisper',
	faction_change: 'Faction change whisper',
	broadcast: 'Scheduled broadcast',
	empty_reset: 'Empty-server map reset',
	risk_kick: 'Kick on connect risk',
	restart_notice: 'Restart notice',
	team_kill: 'Team kill limit'
};

export interface WelcomeConfig {
	message: string;
	onlyFirstVisit: boolean;
	/** wait for the player's first faction pick of the session: they choose one after joining, so a
	 *  whisper on join can land while they are still in the menu */
	afterFaction: boolean;
}
/** Whispers a player when they switch from one faction to another (not their first pick). */
export interface FactionChangeConfig {
	message: string;
}
export interface BroadcastConfig {
	messages: string[];
	everyMinutes: number;
	minPlayers: number;
}
export interface EmptyResetConfig {
	map: string;
	experiences: string[];
	lighting: string;
	zoneAlternator: string;
	afterMinutes: number;
	cooldownMinutes: number;
}
export interface RiskKickConfig {
	vacBans: boolean;
	gameBans: boolean;
	minAccountDays: number;
	privateProfiles: boolean;
	bannedElsewhere: boolean;
	watchlist: boolean;
	spareReserved: boolean;
	reason: string;
}
/**
 * Tells players about the game's own restart: WARDOGS restarts a server once it has been up for
 * twelve hours, at the end of the round then in progress. Two broadcasts per uptime cycle: a
 * heads-up `leadMinutes` before the window opens (0 = none) and `message` once it has, repeated
 * every `repeatMinutes` while the round drags on (0 = once).
 */
export interface RestartNoticeConfig {
	message: string;
	leadMinutes: number;
	leadMessage: string;
	repeatMinutes: number;
	minPlayers: number;
}
/**
 * Acts on team kills the kill feed reports, counted per killer within their current session:
 * a whisper from `warnAt` team kills on (0 = never), a kick at `kickAt` (0 = never).
 */
export interface TeamKillConfig {
	warnAt: number;
	warnMessage: string;
	kickAt: number;
	kickReason: string;
}
export type TriggerConfig =
	| WelcomeConfig
	| FactionChangeConfig
	| BroadcastConfig
	| EmptyResetConfig
	| RiskKickConfig
	| RestartNoticeConfig
	| TeamKillConfig;

const MAX_MESSAGE = 200;

export const isTriggerKind = (v: unknown): v is TriggerKind =>
	TRIGGER_KINDS.includes(v as TriggerKind);

/** Checks and normalises a kind's settings; throws a 400 with a reason people can act on. */
export function validateConfig(kind: TriggerKind, raw: unknown): TriggerConfig {
	const c = (raw && typeof raw === 'object' ? raw : {}) as Record<string, unknown>;
	switch (kind) {
		case 'welcome': {
			const message = str(c.message, MAX_MESSAGE);
			if (!message) throw new ApiError(400, 'The welcome message is empty.');
			return { message, onlyFirstVisit: !!c.onlyFirstVisit, afterFaction: !!c.afterFaction };
		}
		case 'faction_change': {
			const message = str(c.message, MAX_MESSAGE);
			if (!message) throw new ApiError(400, 'The message is empty.');
			return { message };
		}
		case 'broadcast': {
			const list = Array.isArray(c.messages) ? c.messages : String(c.messages ?? '').split('\n');
			const messages = list
				.map((m) => str(m, MAX_MESSAGE))
				.filter(Boolean)
				.slice(0, 20);
			if (!messages.length) throw new ApiError(400, 'Add at least one message to broadcast.');
			const everyMinutes = int(c.everyMinutes, 0, 0, 24 * 60);
			if (!everyMinutes) throw new ApiError(400, 'everyMinutes must be 1-1440.');
			return { messages, everyMinutes, minPlayers: int(c.minPlayers, 1, 0, 1000) };
		}
		case 'empty_reset': {
			const map = str(c.map, 100);
			if (!map) throw new ApiError(400, 'Pick the map to reset to.');
			const experiences = Array.isArray(c.experiences)
				? c.experiences
						.map((e) => str(e, 100))
						.filter(Boolean)
						.slice(0, 10)
				: [];
			const afterMinutes = int(c.afterMinutes, 0, 0, 24 * 60);
			if (!afterMinutes) throw new ApiError(400, 'afterMinutes must be 1-1440.');
			return {
				map,
				experiences,
				lighting: str(c.lighting, 100),
				zoneAlternator: str(c.zoneAlternator, 200),
				afterMinutes,
				cooldownMinutes: int(c.cooldownMinutes, 30, 1, 24 * 60)
			};
		}
		case 'risk_kick': {
			const cfg: RiskKickConfig = {
				vacBans: !!c.vacBans,
				gameBans: !!c.gameBans,
				minAccountDays: int(c.minAccountDays, 0, 0, 3650),
				privateProfiles: !!c.privateProfiles,
				bannedElsewhere: !!c.bannedElsewhere,
				watchlist: !!c.watchlist,
				spareReserved: c.spareReserved === undefined ? true : !!c.spareReserved,
				reason:
					str(c.reason, MAX_MESSAGE) || 'Your account does not meet this server’s requirements.'
			};
			if (
				!cfg.vacBans &&
				!cfg.gameBans &&
				!cfg.minAccountDays &&
				!cfg.bannedElsewhere &&
				!cfg.watchlist
			)
				throw new ApiError(400, 'Turn on at least one rule.');
			return cfg;
		}
		case 'restart_notice': {
			const message = str(c.message, MAX_MESSAGE);
			if (!message) throw new ApiError(400, 'The restart message is empty.');
			const leadMinutes = int(c.leadMinutes, 0, 0, RESTART_AFTER_HOURS * 60 - 1);
			const leadMessage = str(c.leadMessage, MAX_MESSAGE);
			if (leadMinutes && !leadMessage)
				throw new ApiError(400, 'Add the heads-up message, or set the heads-up to 0 minutes.');
			return {
				message,
				leadMinutes,
				leadMessage,
				repeatMinutes: int(c.repeatMinutes, 0, 0, 24 * 60),
				minPlayers: int(c.minPlayers, 1, 0, 1000)
			};
		}
		case 'team_kill': {
			const warnAt = int(c.warnAt, 0, 0, 100);
			const kickAt = int(c.kickAt, 0, 0, 100);
			if (!warnAt && !kickAt)
				throw new ApiError(400, 'Set a whisper threshold, a kick threshold, or both.');
			if (warnAt && kickAt && kickAt < warnAt)
				throw new ApiError(400, 'The kick threshold cannot be below the whisper threshold.');
			return {
				warnAt,
				warnMessage:
					str(c.warnMessage, MAX_MESSAGE) ||
					'Careful, {name}: that was a team kill ({count} this session).',
				kickAt,
				kickReason: str(c.kickReason, MAX_MESSAGE) || 'Team killing ({count} this session).'
			};
		}
	}
}

/** What a team-kill rule does once the killer's count this session has reached `count`. */
export function teamKillStage(
	cfg: Pick<TeamKillConfig, 'warnAt' | 'kickAt'>,
	count: number
): 'kick' | 'warn' | null {
	if (cfg.kickAt && count >= cfg.kickAt) return 'kick';
	if (cfg.warnAt && count >= cfg.warnAt) return 'warn';
	return null;
}

/** Per-server memory of a restart notice: which stages went out for the current game start. */
export interface RestartNoticeState {
	/** the game start (ms) these stamps belong to; a different start is a new cycle */
	startedAt: number;
	leadAt?: number;
	dueAt?: number;
}

export interface RestartNoticeStage {
	stage: 'lead' | 'due';
	/** minutes until the window opens (lead), or 0 once it has */
	minutes: number;
	state: RestartNoticeState;
}

/**
 * Which broadcast a restart notice sends now, if any. The heads-up goes once per game start when
 * the window is `leadMinutes` away or less; the main message once the window is open, and again
 * every `repeatMinutes` when set. A start time the rule has not seen resets both.
 */
export function restartNoticeStage(
	cfg: Pick<RestartNoticeConfig, 'leadMinutes' | 'repeatMinutes' | 'minPlayers'>,
	prev: RestartNoticeState | null | undefined,
	input: { startedAt: number; playerCount: number; now: number }
): RestartNoticeStage | null {
	if (!input.startedAt || input.playerCount < cfg.minPlayers) return null;
	const w = restartWindow(new Date(input.startedAt).toISOString(), RESTART_AFTER_HOURS, input.now);
	if (!w) return null;
	const state: RestartNoticeState =
		prev && prev.startedAt === input.startedAt ? { ...prev } : { startedAt: input.startedAt };
	if (w.due) {
		const again =
			cfg.repeatMinutes > 0 && input.now - (state.dueAt ?? 0) >= cfg.repeatMinutes * 60_000;
		if (state.dueAt && !again) return null;
		state.dueAt = input.now;
		return { stage: 'due', minutes: 0, state };
	}
	if (!cfg.leadMinutes || state.leadAt || w.untilDueMs === null) return null;
	if (w.untilDueMs > cfg.leadMinutes * 60_000) return null;
	state.leadAt = input.now;
	return { stage: 'lead', minutes: Math.max(1, Math.round(w.untilDueMs / 60_000)), state };
}

/** A player who has a faction now and did not have this one at the last look. */
export interface FactionPick<P> {
	player: P;
	/** the faction they had before; null for their first pick of the session (or they arrived with one) */
	from: string | null;
}

/** Who a welcome rule whispers on this tick: joiners, or first faction picks when it waits for them. */
export function welcomeTargets<P extends { steamId: string }>(
	cfg: Pick<WelcomeConfig, 'onlyFirstVisit' | 'afterFaction'>,
	tick: { joined: P[]; factioned: FactionPick<P>[]; firstVisit: Set<string> }
): P[] {
	const pool = cfg.afterFaction
		? tick.factioned.filter((f) => !f.from).map((f) => f.player)
		: tick.joined;
	return cfg.onlyFirstVisit ? pool.filter((p) => tick.firstVisit.has(p.steamId)) : pool;
}

/** Who a faction-change rule whispers: players who switched from one faction to another. */
export const factionChangeTargets = <P>(tick: { factioned: FactionPick<P>[] }): FactionPick<P>[] =>
	tick.factioned.filter((f) => !!f.from);

/** Fills {name}, {faction}, {previous}, {server}, {map}, {players} and {max}; unknown ones stay. */
export function renderTemplate(text: string, vars: Record<string, string | number>): string {
	const lower: Record<string, string> = {};
	for (const [k, v] of Object.entries(vars)) lower[k.toLowerCase()] = String(v);
	return text
		.replace(/\{([a-z_]+)\}/gi, (m, key: string) => lower[key.toLowerCase()] ?? m)
		.slice(0, MAX_MESSAGE);
}

export interface RiskKickSignals {
	profile: SteamProfileRow | null;
	steamEnabled: boolean;
	bannedOn: { serverName: string; reason: string }[];
	watched: { reason: string } | null;
	reserved: boolean;
	now?: Date;
}

/** Why a joiner would be kicked under this config, or null when they pass. */
export function riskKickVerdict(cfg: RiskKickConfig, s: RiskKickSignals): string | null {
	if (s.reserved && cfg.spareReserved) return null;
	if (cfg.bannedElsewhere && s.bannedOn.length)
		return `banned on ${s.bannedOn[0].serverName}${s.bannedOn[0].reason ? ` (${s.bannedOn[0].reason})` : ''}`;
	if (cfg.watchlist && s.watched)
		return `on the watchlist${s.watched.reason ? ` (${s.watched.reason})` : ''}`;
	if (s.steamEnabled && s.profile && !s.profile.error) {
		const p = s.profile;
		if (cfg.vacBans && p.vacBans > 0)
			return `${p.vacBans} VAC ban${p.vacBans === 1 ? '' : 's'} on record`;
		if (cfg.gameBans && p.gameBans > 0)
			return `${p.gameBans} game ban${p.gameBans === 1 ? '' : 's'} on record`;
		if (cfg.minAccountDays > 0) {
			const age = accountAgeDays(p.accountCreatedAt, s.now);
			if (age === null) {
				if (cfg.privateProfiles) return 'private profile, account age unknown';
			} else if (age < cfg.minAccountDays) {
				return `Steam account only ${age} day${age === 1 ? '' : 's'} old (minimum ${cfg.minAccountDays})`;
			}
		}
	}
	return null;
}

const sameExperiences = (a: string[], b: string[]) =>
	[...a].sort().join('+') === [...b].sort().join('+');

/** Is the server already on the reset target? Map, and the experiences when the rule names any. */
export const onTarget = (
	cfg: Pick<EmptyResetConfig, 'map' | 'experiences'>,
	current: { map: string; experiences: string[] }
) =>
	current.map === cfg.map &&
	(!cfg.experiences.length || sameExperiences(cfg.experiences, current.experiences));
