// The public surface: which servers anyone may see, and the reads the public pages and JSON
// share. Every gate lives here so a route cannot drift: a page that is off answers 404, never
// 403, so a closed page looks like no page. Reads never reach the worker or Steam: the live
// snapshot row, the history tables and the Steam cache only, each behind a per-address limit.
import { and, asc, eq, isNull } from 'drizzle-orm';
import { error, type RequestEvent } from '@sveltejs/kit';
import type { Env } from './env';
import { ApiError, clientIp, normalizeError } from './http';
import { organizations, servers, type OrgRow, type ServerRow } from './db/schema';
import { readLiveRows } from './live';
import { recentKills } from './feed';
import { assertRate } from './ratelimit';
import { effectiveFeatures, type FeatureSet, type PublicFeature } from '$lib/features';
import { EMPTY_FILTER } from '$lib/kills';
import type { KillView, LiveView } from '$lib/types';

export interface PublicServer {
	server: ServerRow;
	org: OrgRow;
	features: FeatureSet;
}

const notFound = () => new ApiError(404, 'Not found.', 'not_found');
const PLAIN = /^[A-Za-z0-9_-]{1,64}$/;

/** The server with `feature` on (allowed by the site owner, switched on by the org, org not suspended); 404 otherwise. */
export async function requirePublicServer(
	env: Env,
	id: string,
	feature: PublicFeature
): Promise<PublicServer> {
	if (!PLAIN.test(id)) throw notFound();
	const [row] = await env.db
		.select({ server: servers, org: organizations })
		.from(servers)
		.innerJoin(organizations, eq(organizations.id, servers.orgId))
		.where(and(eq(servers.id, id), isNull(organizations.suspendedAt)))
		.limit(1);
	if (!row) throw notFound();
	const features = effectiveFeatures(row.org, row.server);
	if (!features[feature]) throw notFound();
	return { ...row, features };
}

/** The org's servers with `feature` on, dashboard order: what a public board's org scope covers. */
export async function publicOrgServers(
	env: Env,
	org: OrgRow,
	feature: PublicFeature
): Promise<ServerRow[]> {
	const rows = await env.db
		.select()
		.from(servers)
		.where(eq(servers.orgId, org.id))
		.orderBy(asc(servers.sortOrder), asc(servers.name));
	return rows.filter((s) => effectiveFeatures(org, s)[feature]);
}

/** What the public status page shows: never a Steam ID, ping, cash, the build or an error text. */
export interface PublicStatus {
	serverId: string;
	name: string;
	orgName: string;
	/** reachable at the last look */
	ok: boolean;
	observedAt: string | null;
	joinCode: string | null;
	map: string | null;
	lighting: string | null;
	experiences: string[];
	players: number;
	maxPlayers: number | null;
	reservedSlots: number | null;
	scores: { name: string; colorHex: string; score: number }[];
	scoreCap: number | null;
	matchSeconds: number | null;
	/** steamId is set only while the server's career pages are open: it is the link, never shown */
	roster: {
		name: string;
		faction: string | null;
		kills: number;
		deaths: number;
		steamId: string | null;
	}[];
	/** the last kills, newest first; null when the server does not show its feed publicly */
	kills: PublicKill[] | null;
}

/** One kill as the public page shows it: names and factions, never a SteamID. */
export interface PublicKill {
	eventId: string;
	ts: string;
	/** seconds on the match clock */
	eventTime: number;
	/** null: the environment */
	killer: { name: string; faction: string | null } | null;
	victim: { name: string; faction: string | null };
	/** the raw weapon or vehicle tag; $lib/causes labels it */
	cause: string | null;
	distanceM: number | null;
	headshot: boolean;
	suicide: boolean;
	teamKill: boolean;
	tags: string[];
}

/** How many kills the public page carries: enough to read the last few minutes, one small read. */
export const PUBLIC_KILLS = 20;

export const publicKill = (k: KillView): PublicKill => ({
	eventId: k.eventId,
	ts: k.ts,
	eventTime: k.eventTime,
	killer: k.killer ? { name: k.killer.name, faction: k.killer.faction } : null,
	victim: { name: k.victim.name, faction: k.victim.faction },
	cause: k.cause,
	distanceM: k.distanceM,
	headshot: k.headshot,
	suicide: k.suicide,
	teamKill: k.teamKill,
	tags: k.tags
});

/**
 * The public shape of the live snapshot. An unreachable server keeps its map (the art stays
 * up) and says only that it could not be reached: the stored error names the RCON host and
 * port and never leaves the panel.
 */
export function publicStatus(
	ps: PublicServer,
	live: LiveView | null,
	kills: KillView[] | null
): PublicStatus {
	const ok = !!live && live.ok && !!live.status;
	const s = live?.status ?? null;
	const linkable = ps.features.leaderboards;
	return {
		serverId: ps.server.id,
		name: ps.server.name,
		orgName: ps.org.name,
		ok,
		observedAt: live?.observedAt ?? null,
		joinCode: live?.gameServerId || null,
		map: s?.map || null,
		lighting: s?.lighting || null,
		experiences: s ? [...s.experiences] : [],
		players: ok ? s!.playerCount : 0,
		maxPlayers: ok ? s!.maxPlayers : null,
		reservedSlots: live?.reservedSlots ?? null,
		scores: ok
			? s!.scores.map((f) => ({ name: f.name, colorHex: f.colorHex, score: f.score }))
			: [],
		scoreCap: ok ? s!.scoreCap : null,
		matchSeconds: ok ? s!.matchSeconds : null,
		roster: ok
			? live!.players
					.map((p) => ({
						name: String(p.name ?? ''),
						faction: p.faction ?? null,
						kills: Number(p.kills) || 0,
						deaths: Number(p.deaths) || 0,
						steamId: linkable && /^\d{17}$/.test(String(p.steamId ?? '')) ? String(p.steamId) : null
					}))
					.sort((a, b) => b.kills - a.kills || a.deaths - b.deaths || a.name.localeCompare(b.name))
			: [],
		kills: kills ? kills.map(publicKill) : null
	};
}

/**
 * The live snapshot row (never a worker call) and, when the server shows its feed, the last
 * kills by the same index the Kills tab pages on: two small reads however many viewers ask.
 */
export async function readPublicStatus(env: Env, ps: PublicServer): Promise<PublicStatus> {
	const [rows, kills] = await Promise.all([
		readLiveRows(env, [ps.server.id]),
		ps.server.publicKills ? recentKills(env, ps.server.id, null, PUBLIC_KILLS, EMPTY_FILTER) : null
	]);
	return publicStatus(ps, rows.get(ps.server.id) ?? null, kills);
}

/** What the public shell shows above every page of a server. */
export const publicHeading = (ps: PublicServer) => ({
	id: ps.server.id,
	name: ps.server.name,
	orgName: ps.org.name,
	discordInviteUrl: ps.org.discordInviteUrl,
	features: ps.features
});

// --- limits, per client address and per process ---

/** Forty viewers behind one address polling every twenty seconds fit; a scraper does not. */
export const PUBLIC_READS_PER_MINUTE = 120;

export const limitPublicReads = (req: Request): void =>
	assertRate(`public:${clientIp(req)}`, PUBLIC_READS_PER_MINUTE, 60_000);

/** Public JSON may sit in a shared cache for a few seconds; the panel's own JSON never does. */
export const publicHeaders = (maxAge: number): Record<string, string> => ({
	'cache-control': `public, max-age=${maxAge}`
});

/** A thrown ApiError (404, 429) as the page error SvelteKit renders. */
export function pageFail(err: unknown): never {
	const known = normalizeError(err);
	if (!known) throw err;
	error(known.status, known.message);
}

/** Runs a public page's load behind the rate limit, turning refusals into page errors. */
export async function publicLoad<T>(
	event: Pick<RequestEvent, 'request'>,
	fn: () => Promise<T>
): Promise<T> {
	try {
		limitPublicReads(event.request);
		return await fn();
	} catch (err) {
		return pageFail(err);
	}
}
