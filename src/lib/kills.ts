// The kill feed's filter: what the Kills tab asks for, how it travels in a query string, and
// whether one kill matches it. The API applies the same filter in SQL (feed.ts); the page applies
// this one to kills arriving live, so the two must agree. Client-safe.
import { causeKind } from './causes';
import type { KillView } from './types';

export type KillKind = '' | 'headshot' | 'teamKill' | 'suicide' | 'vehicle' | 'environment';

export interface KillFilter {
	/** either side: a SteamID exactly, or part of a name */
	player: string;
	killer: string;
	victim: string;
	/** the raw cause tag, exactly */
	cause: string;
	kind: KillKind;
	/** at least this far, in metres */
	minM: number | null;
}

export const EMPTY_FILTER: KillFilter = {
	player: '',
	killer: '',
	victim: '',
	cause: '',
	kind: '',
	minM: null
};

export const KINDS: { key: KillKind; label: string }[] = [
	{ key: '', label: 'Any kind' },
	{ key: 'headshot', label: 'Headshots' },
	{ key: 'teamKill', label: 'Team kills' },
	{ key: 'suicide', label: 'Suicides' },
	{ key: 'vehicle', label: 'By vehicle' },
	{ key: 'environment', label: 'Falls and the environment' }
];
const KIND_KEYS = new Set<string>(KINDS.map((k) => k.key));

const STEAM_RE = /^\d{17}$/;
/** The tags that mean a vehicle did it even when the cause is a weapon or missing. */
export const VEHICLE_TAGS = ['VehicleExplosion', 'RoadKill'];

const text = (v: string | null, max = 100): string => (v ?? '').trim().slice(0, max);

/** The filter a query string carries; anything unknown or malformed is simply not a filter. */
export function parseKillFilter(params: URLSearchParams): KillFilter {
	const kind = params.get('kind') ?? '';
	const min = Number(params.get('minM'));
	return {
		player: text(params.get('player')),
		killer: text(params.get('killer')),
		victim: text(params.get('victim')),
		cause: text(params.get('cause'), 200),
		kind: KIND_KEYS.has(kind) ? (kind as KillKind) : '',
		minM: params.get('minM') && Number.isFinite(min) && min > 0 ? Math.round(min) : null
	};
}

/** The filter as query parameters, blanks left out (for `qs()` and the page's URL). */
export function killFilterParams(f: KillFilter): Record<string, string> {
	const out: Record<string, string> = {};
	if (f.player) out.player = f.player;
	if (f.killer) out.killer = f.killer;
	if (f.victim) out.victim = f.victim;
	if (f.cause) out.cause = f.cause;
	if (f.kind) out.kind = f.kind;
	if (f.minM !== null) out.minM = String(f.minM);
	return out;
}

export const isEmptyFilter = (f: KillFilter): boolean =>
	Object.keys(killFilterParams(f)).length === 0;

/** A SteamID matches exactly; anything else matches part of the name, case-insensitively. */
export function sideMatches(
	needle: string,
	side: { steamId: string; name: string } | null
): boolean {
	if (!needle) return true;
	if (!side) return false;
	if (STEAM_RE.test(needle)) return side.steamId === needle;
	return side.name.toLowerCase().includes(needle.toLowerCase());
}

/** Whether a kill is one the filter asks for; mirrors the SQL in feed.ts. */
export function killMatches(f: KillFilter, k: KillView): boolean {
	if (!sideMatches(f.killer, k.killer) || !sideMatches(f.victim, k.victim)) return false;
	if (f.player && !sideMatches(f.player, k.killer) && !sideMatches(f.player, k.victim))
		return false;
	if (f.cause && k.cause !== f.cause) return false;
	if (f.minM !== null && (k.distanceM === null || k.distanceM < f.minM)) return false;
	switch (f.kind) {
		case 'headshot':
			return k.headshot;
		case 'teamKill':
			return k.teamKill;
		case 'suicide':
			return k.suicide;
		case 'environment':
			return k.killer === null;
		case 'vehicle': {
			const kind = causeKind(k.cause);
			return (
				kind === 'vehicle' ||
				kind === 'vehicle weapon' ||
				k.tags.some((t) => VEHICLE_TAGS.includes(t))
			);
		}
		default:
			return true;
	}
}
