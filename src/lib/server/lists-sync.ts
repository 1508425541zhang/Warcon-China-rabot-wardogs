// The list sync: pushes each org's ban and reserved-slot lists, and each server's own reserved
// slots, to its game servers. The worker runs it on a schedule inside its observations (planning
// against the snapshot it keeps in server_bans and server_reserved, and re-reading the server
// before it changes anything); the API runs it right after an admin edits a list, so the toast
// can say where the change landed.
//
// Rules of the road: the panel adds what the lists want and removes only what it added itself
// (server_list_state). Every game call is idempotent in the panel's reading of it ("already
// banned" is a success, "not banned" on delete is a success), so two replicas working the same
// server at once do no harm; the in-process lock below only keeps the poller and an API call in
// one process from interleaving.
import { and, eq, gt, inArray, isNotNull, isNull, lte, notInArray, or, sql } from 'drizzle-orm';
import type { Env } from './env';
import { banUid, renderBanMessage } from '$lib/ban-message';
import { publicMessage } from './http';
import { writeAudit } from './audit';
import { ACTIONS } from './actions';
import { withServer, type Priority } from './dispatcher';
import { GameError, WardogsClient } from './rcon';
import {
	listEntries,
	lists,
	organizations,
	orgMembers,
	serverBans,
	serverListState,
	serverListSync,
	serverLists,
	serverReserved,
	servers,
	user,
	type OrgRow,
	type ServerRow
} from './db/schema';
import {
	activeEntries,
	desiredOf,
	isAlreadyApplied,
	isGone,
	isUnreachable,
	planHasWork,
	planSync,
	type Kind,
	type PlanInput,
	type PanelBan,
	type SyncPlan
} from './lists-plan';
import type { DbOrTx } from './db';
import type { Ban, Features, ListSyncServer, ListSyncSummary } from '$lib/types';

/** A failed add or remove is not retried for this long. */
const RETRY_AFTER_MS = 5 * 60_000;
/** How long an API-triggered fan-out waits for each server before reporting it as still syncing. */
const FANOUT_WAIT_MS = 15_000;

export interface Observed {
	bans: Ban[];
	reserved: string[];
}

export interface SyncResult extends ListSyncServer {
	/** why nothing ran, when nothing ran */
	skipped?: 'busy' | 'suspended';
	/** the server's lists after the run; the poller keeps its in-memory copy from this */
	observed?: { bans: string[]; reserved: string[] };
	/** the bans the lists put on this server; the worker removes these players on sight */
	bans?: PanelBan[];
}

// ---- per-server lock ---------------------------------------------------------------------------

const locks = new Map<string, Promise<void>>();

/** Runs fn while holding this process's lock on the server; undefined if it was busy for longer than waitMs. */
export async function withServerLock<T>(
	serverId: string,
	waitMs: number,
	fn: () => Promise<T>
): Promise<T | undefined> {
	const deadline = Date.now() + waitMs;
	for (;;) {
		const held = locks.get(serverId);
		if (!held) break;
		const left = deadline - Date.now();
		if (left <= 0) return undefined;
		const outcome = await Promise.race([
			held.then(() => 'free' as const),
			new Promise<'timeout'>((r) => setTimeout(() => r('timeout'), left))
		]);
		if (outcome === 'timeout') return undefined;
	}
	let release!: () => void;
	locks.set(serverId, new Promise<void>((r) => (release = r)));
	try {
		return await fn();
	} finally {
		locks.delete(serverId);
		release();
	}
}

// ---- desired and observed ----------------------------------------------------------------------

/** What the lists this server subscribes to want on it right now. */
export async function desiredFor(
	env: Env,
	server: Pick<ServerRow, 'id' | 'orgId'>,
	org: Pick<OrgRow, 'membersReserved' | 'banMessage'>,
	now = new Date()
): Promise<PlanInput['desired']> {
	const rows = await env.db
		.select({ e: listEntries, kind: lists.kind, listServerId: lists.serverId })
		.from(serverLists)
		.innerJoin(lists, eq(lists.id, serverLists.listId))
		.innerJoin(listEntries, eq(listEntries.listId, lists.id))
		.where(and(eq(serverLists.serverId, server.id), isNull(listEntries.removedAt)));
	const active = activeEntries(
		rows.map((r) => ({ ...r.e, kind: r.kind, serverId: r.listServerId })),
		now
	);
	const { bans, reserved } = desiredOf(active);
	if (org.membersReserved) {
		// Members who set a SteamID get a slot from the org's reserve list, unless the org has
		// banned them.
		const [reserveList] = await env.db
			.select({ id: lists.id })
			.from(serverLists)
			.innerJoin(lists, eq(lists.id, serverLists.listId))
			.where(
				and(eq(serverLists.serverId, server.id), eq(lists.kind, 'reserve'), isNull(lists.serverId))
			)
			.limit(1);
		if (reserveList) {
			const banned = new Set(bans.map((b) => b.steamId));
			const have = new Set(reserved.map((r) => r.steamId));
			for (const m of await memberSlots(env, server.orgId))
				if (!banned.has(m.steamId) && !have.has(m.steamId))
					reserved.push({ steamId: m.steamId, listId: reserveList.id, member: true });
		}
	}
	return { bans, reserved };
}

/** Members of the org who linked a SteamID on their account and are not disabled. */
export async function memberSlots(
	env: Env,
	orgId: string
): Promise<{ steamId: string; userId: string; username: string; since: Date }[]> {
	const rows = await env.db
		.select({
			steamId: user.steamId,
			userId: user.id,
			username: user.username,
			since: orgMembers.createdAt
		})
		.from(orgMembers)
		.innerJoin(user, eq(user.id, orgMembers.userId))
		.where(
			and(
				eq(orgMembers.orgId, orgId),
				isNotNull(user.steamId),
				or(isNull(user.banned), eq(user.banned, false))
			)
		)
		.orderBy(orgMembers.createdAt);
	return rows.map((r) => ({
		steamId: r.steamId!,
		userId: r.userId,
		username: r.username || '',
		since: r.since
	}));
}

/**
 * Lifts bans and reserved slots whose expiry has passed: the row is marked removed (so history
 * keeps it) and the next reconcile takes it off every server the panel applied it to. The poller runs this every
 * tick and fanOut before pushing, so an install without a poller still catches up on edit.
 */
export async function expireEntries(env: Env): Promise<{ lifted: number; orgIds: string[] }> {
	const now = new Date();
	const rows = await env.db
		.update(listEntries)
		.set({ removedAt: now, removedByName: 'expiry', removal: 'expired' })
		.where(
			and(
				isNull(listEntries.removedAt),
				isNotNull(listEntries.expiresAt),
				lte(listEntries.expiresAt, now)
			)
		)
		.returning({ listId: listEntries.listId, steamId: listEntries.steamId });
	if (!rows.length) return { lifted: 0, orgIds: [] };
	const listIds = [...new Set(rows.map((r) => r.listId))];
	const owners = await env.db
		.select({ listId: lists.id, kind: lists.kind, orgId: lists.orgId, orgName: organizations.name })
		.from(lists)
		.innerJoin(organizations, eq(organizations.id, lists.orgId))
		.where(inArray(lists.id, listIds));
	await env.db.update(lists).set({ updatedAt: now }).where(inArray(lists.id, listIds));
	for (const o of owners) {
		const ids = rows.filter((r) => r.listId === o.listId).map((r) => r.steamId);
		await writeAudit(env, null, {
			actorName: 'list sync',
			orgId: o.orgId,
			category: 'system',
			action: 'list.expire',
			target: ids.join(', '),
			outcome: 'ok',
			message: `${ids.length} ${o.kind === 'ban' ? 'ban' : 'reserved slot'}${ids.length === 1 ? '' : 's'} expired in ${o.orgName}`,
			detail: { orgId: o.orgId, org: o.orgName, steamIds: ids }
		}).catch((err) => console.error('[warcon] list.expire audit', err));
	}
	return { lifted: rows.length, orgIds: [...new Set(owners.map((o) => o.orgId))] };
}

async function snapshotObserved(env: Env, serverId: string): Promise<Observed> {
	const [bans, reserved] = await Promise.all([
		env.db.select().from(serverBans).where(eq(serverBans.serverId, serverId)),
		env.db
			.select({ steamId: serverReserved.steamId })
			.from(serverReserved)
			.where(eq(serverReserved.serverId, serverId))
	]);
	return {
		bans: bans.map((b) => ({
			steamId: b.steamId,
			reason: b.reason,
			bannedBy: b.bannedBy,
			bannedAtUtc: b.bannedAtUtc
		})),
		reserved: reserved.map((r) => r.steamId)
	};
}

/** Reads the server's ban list and reserved slots, one request at a time (one in flight per server). */
export async function liveObserved(client: WardogsClient): Promise<Observed> {
	const bans = (await ACTIONS.bans.run(client, {})) as { bans: Ban[] };
	const reserved = (await ACTIONS.reserved.run(client, {})) as { reserved: string[] };
	return {
		bans: bans.bans.filter((b) => /^\d{17}$/.test(b.steamId)),
		reserved: reserved.reserved.filter((id) => /^\d{17}$/.test(id))
	};
}

/** Rewrites the poller's copies of a server's ban list and reserved slots. */
export async function writeSnapshot(
	env: Env,
	serverId: string,
	observed: Observed,
	ts = new Date()
): Promise<void> {
	await env.db.transaction(async (tx) => {
		const banIds = observed.bans.map((b) => b.steamId);
		await tx
			.delete(serverBans)
			.where(
				banIds.length
					? and(eq(serverBans.serverId, serverId), notInArray(serverBans.steamId, banIds))
					: eq(serverBans.serverId, serverId)
			);
		if (banIds.length)
			await tx
				.insert(serverBans)
				.values(
					observed.bans.map((b) => ({
						serverId,
						steamId: b.steamId,
						reason: b.reason || '',
						bannedBy: b.bannedBy || '',
						bannedAtUtc: b.bannedAtUtc || '',
						seenAt: ts
					}))
				)
				.onConflictDoUpdate({
					target: [serverBans.serverId, serverBans.steamId],
					set: {
						reason: sql`excluded.reason`,
						bannedBy: sql`excluded.banned_by`,
						bannedAtUtc: sql`excluded.banned_at_utc`,
						seenAt: ts
					}
				});
		await tx
			.delete(serverReserved)
			.where(
				observed.reserved.length
					? and(
							eq(serverReserved.serverId, serverId),
							notInArray(serverReserved.steamId, observed.reserved)
						)
					: eq(serverReserved.serverId, serverId)
			);
		if (observed.reserved.length)
			await tx
				.insert(serverReserved)
				.values(observed.reserved.map((steamId) => ({ serverId, steamId, seenAt: ts })))
				.onConflictDoUpdate({
					target: [serverReserved.serverId, serverReserved.steamId],
					set: { seenAt: ts }
				});
	});
}

/**
 * A ban or reserved slot someone just added or removed by hand through the rcon actions: the
 * copies in server_bans and server_reserved are brought in line at once, rather than at the
 * worker's next re-read (five minutes by default), so no page keeps showing a slot the server no
 * longer has, or misses one it just got. The worker's own re-read still follows and is the
 * authority; this only closes the gap.
 */
export async function noteLocalEdit(
	env: Env,
	serverId: string,
	kind: Kind,
	op: 'add' | 'remove',
	steamId: string,
	reason = '',
	ts = new Date()
): Promise<void> {
	if (kind === 'ban') {
		if (op === 'remove') {
			await env.db
				.delete(serverBans)
				.where(and(eq(serverBans.serverId, serverId), eq(serverBans.steamId, steamId)));
			return;
		}
		await env.db
			.insert(serverBans)
			.values({ serverId, steamId, reason, seenAt: ts })
			.onConflictDoUpdate({
				target: [serverBans.serverId, serverBans.steamId],
				set: { reason, seenAt: ts }
			});
		return;
	}
	if (op === 'remove') {
		await env.db
			.delete(serverReserved)
			.where(and(eq(serverReserved.serverId, serverId), eq(serverReserved.steamId, steamId)));
		return;
	}
	await env.db
		.insert(serverReserved)
		.values({ serverId, steamId, seenAt: ts })
		.onConflictDoUpdate({
			target: [serverReserved.serverId, serverReserved.steamId],
			set: { seenAt: ts }
		});
}

// ---- the run -----------------------------------------------------------------------------------

export interface ReconcileOptions {
	/** the worker's cached feature flags, so the run need not ask the build again; null = unknown */
	features?: Features | null;
	reason: 'poll' | 'api';
	/** how long to wait for this process's lock on the server; 0 = skip if busy */
	waitMs: number;
	client?: WardogsClient;
	/** the server's lists as just read by the caller (the poller's periodic refresh) */
	observed?: Observed;
	/**
	 * How to get at the server: a dispatcher priority to queue for its lane, or 'held' when the
	 * caller already holds the lane (the worker, inside an observation).
	 */
	lane: Priority | 'held';
}

const failure = (err: unknown) => {
	const g = err instanceof GameError ? err : null;
	return { status: g?.status ?? 500, code: g?.code, message: publicMessage(err, 'Failed.') };
};

/** Brings one server in line with its lists. Never throws for game-side trouble; records it instead. */
export async function reconcileServer(
	env: Env,
	server: ServerRow,
	org: OrgRow,
	opts: ReconcileOptions
): Promise<SyncResult> {
	const base: SyncResult = {
		serverId: server.id,
		serverName: server.name,
		ok: false,
		added: 0,
		removed: 0,
		failed: 0,
		pending: false,
		error: ''
	};
	if (org.suspendedAt) return { ...base, skipped: 'suspended', error: 'Organisation suspended.' };
	const locked = () =>
		withServerLock(server.id, opts.waitMs, () => run(env, server, org, opts, base));
	const ran =
		opts.lane === 'held' ? await locked() : await withServer(server.id, opts.lane, locked);
	return ran ?? { ...base, pending: true, skipped: 'busy', error: 'Sync already running.' };
}

async function run(
	env: Env,
	server: ServerRow,
	org: OrgRow,
	opts: ReconcileOptions,
	base: SyncResult
): Promise<SyncResult> {
	const now = new Date();
	const desired = await desiredFor(env, server, org, now);
	const panelBans = desired.bans.map(({ steamId, listId }) => ({ steamId, listId }));
	const state = await env.db
		.select()
		.from(serverListState)
		.where(eq(serverListState.serverId, server.id));
	const [syncRow] = await env.db
		.select()
		.from(serverListSync)
		.where(eq(serverListSync.serverId, server.id))
		.limit(1);

	const planWith = (observed: Observed) =>
		planSync({
			now,
			retryAfterMs: RETRY_AFTER_MS,
			desired,
			observed: { reserved: observed.reserved },
			state
		});

	// Plan against what we last saw; before touching the server, look again.
	let observed = opts.observed ?? (await snapshotObserved(env, server.id));
	let fresh = !!opts.observed;
	let plan = planWith(observed);
	let client = opts.client;
	let reserve: ReserveMode = { writable: true, viaConfig: false };
	if (!planHasWork(plan) && plan.confirms.length === 0 && plan.deletes.length === 0) {
		await bookkeep(env, server.id, { syncedAt: now, lastError: '' });
		return {
			...base,
			ok: true,
			observed: flat(observed),
			bans: panelBans
		};
	}
	try {
		client ??= await WardogsClient.forServer(env, server);
		if (!fresh) {
			observed = await liveObserved(client);
			fresh = true;
			await writeSnapshot(env, server.id, observed, now);
		}
		plan = planWith(observed);
		// Live builds since CL-499480 have no reserved-slot routes and answer those calls 404, which
		// would otherwise read as "already gone"; there the slots go through the config document
		// (see reservedViaConfig in actions.ts). Ask the build once before touching reserved slots.
		if ([...plan.adds, ...plan.removes].some((x) => x.kind === 'reserve')) {
			try {
				const features =
					opts.features ??
					((await ACTIONS.capabilities.run(client, {})) as { features: Features }).features;
				reserve = {
					writable: features.reservedSlots || features.configDocument,
					viaConfig: !features.reservedSlots
				};
			} catch {
				/* a build too old to report capabilities still has the routes */
			}
		}
	} catch (err) {
		const message = publicMessage(err, 'Could not reach the server.');
		await bookkeep(env, server.id, { syncedAt: syncRow?.syncedAt ?? null, lastError: message });
		return { ...base, error: message };
	}

	const outcome = await execute(client, plan, observed, reserve);
	await record(env, server.id, plan, outcome, now, {
		syncedAt: now,
		lastError: outcome.aborted ?? ''
	});

	const failedNow = [...outcome.failedAdds, ...outcome.failedRemoves];
	const previous = new Map(state.map((s) => [`${s.kind}:${s.steamId}`, s.error]));
	const newFailures = failedNow.filter((f) => previous.get(`${f.kind}:${f.steamId}`) !== f.error);
	if (outcome.added.length || outcome.removed.length || newFailures.length || outcome.aborted) {
		const parts: string[] = [];
		if (outcome.added.length) parts.push(`${outcome.added.length} added`);
		if (outcome.removed.length) parts.push(`${outcome.removed.length} removed`);
		if (failedNow.length) parts.push(`${failedNow.length} failed`);
		if (outcome.aborted) parts.push(`stopped: ${outcome.aborted}`);
		await writeAudit(env, null, {
			actorName: 'list sync',
			server: { id: server.id, name: server.name },
			orgId: server.orgId,
			category: 'system',
			action: 'lists.sync',
			target: org.name,
			outcome: failedNow.length || outcome.aborted ? 'error' : 'ok',
			status: failedNow.length || outcome.aborted ? 502 : 200,
			message: parts.join(', '),
			detail: {
				reason: opts.reason,
				added: outcome.added.map(refOf),
				removed: outcome.removed.map(refOf),
				failed: failedNow.map((f) => ({ kind: f.kind, steamId: f.steamId, error: f.error }))
			}
		}).catch((err) => console.error('[warcon] lists.sync audit', err));
	}
	return {
		...base,
		ok: true,
		added: outcome.added.length,
		removed: outcome.removed.length,
		failed: failedNow.length,
		error: outcome.aborted ?? '',
		observed: flat(outcome.observed),
		bans: panelBans
	};
}

const refOf = (r: { kind: Kind; steamId: string }) => `${r.kind}:${r.steamId}`;
const flat = (o: Observed) => ({ bans: o.bans.map((b) => b.steamId), reserved: o.reserved });

interface Failed {
	kind: Kind;
	steamId: string;
	listId?: string;
	error: string;
}

interface Outcome {
	added: SyncPlan['adds'];
	removed: SyncPlan['removes'];
	failedAdds: (Failed & { listId: string })[];
	failedRemoves: Failed[];
	/** the server stopped answering part-way; the rest was not attempted */
	aborted: string | null;
	observed: Observed;
}

const NO_RESERVED_ROUTES =
	'This server build has no reserved-slot routes and its config document is not writable, so the panel cannot place reserved slots on it.';

/** How reserved slots reach this server: live routes, the config document, or not at all. */
interface ReserveMode {
	writable: boolean;
	/** no live routes: tell the actions to go straight to the document */
	viaConfig: boolean;
}

/** Removes, then adds, one call at a time; stops at the first sign the server is gone. */
async function execute(
	client: WardogsClient,
	plan: SyncPlan,
	before: Observed,
	reserve: ReserveMode = { writable: true, viaConfig: false }
): Promise<Outcome> {
	const out: Outcome = {
		added: [],
		removed: [],
		failedAdds: [],
		failedRemoves: [],
		aborted: null,
		observed: { bans: [...before.bans], reserved: [...before.reserved] }
	};
	const dropObserved = (kind: Kind, steamId: string) => {
		if (kind === 'ban') out.observed.bans = out.observed.bans.filter((b) => b.steamId !== steamId);
		else out.observed.reserved = out.observed.reserved.filter((id) => id !== steamId);
	};
	const addObserved = (kind: Kind, steamId: string, reason: string) => {
		if (kind === 'ban') {
			if (!out.observed.bans.some((b) => b.steamId === steamId))
				out.observed.bans.push({
					steamId,
					reason,
					bannedBy: 'Warcon',
					bannedAtUtc: new Date().toISOString()
				});
		} else if (!out.observed.reserved.includes(steamId)) out.observed.reserved.push(steamId);
	};
	for (const r of plan.removes) {
		if (r.kind === 'reserve' && !reserve.writable) {
			out.failedRemoves.push({ ...r, error: `Could not remove: ${NO_RESERVED_ROUTES}` });
			continue;
		}
		try {
			await ACTIONS.reservedRemove.run(client, {
				steamId: r.steamId,
				viaConfig: reserve.viaConfig
			});
			out.removed.push(r);
			dropObserved(r.kind, r.steamId);
		} catch (err) {
			const f = failure(err);
			if (isGone(f)) {
				out.removed.push(r);
				dropObserved(r.kind, r.steamId);
			} else if (isUnreachable(f)) {
				out.aborted = f.message;
				return out;
			} else out.failedRemoves.push({ ...r, error: `Could not remove: ${f.message}` });
		}
	}
	for (const a of plan.adds) {
		if (a.kind === 'reserve' && !reserve.writable) {
			out.failedAdds.push({ ...a, error: `Could not add: ${NO_RESERVED_ROUTES}` });
			continue;
		}
		try {
			await ACTIONS.reservedAdd.run(client, {
				steamId: a.steamId,
				viaConfig: reserve.viaConfig
			});
			out.added.push(a);
			addObserved(a.kind, a.steamId, a.reason);
		} catch (err) {
			const f = failure(err);
			if (isAlreadyApplied(f)) {
				out.added.push(a);
				addObserved(a.kind, a.steamId, a.reason);
			} else if (isUnreachable(f)) {
				out.aborted = f.message;
				return out;
			} else out.failedAdds.push({ ...a, error: f.message });
		}
	}
	return out;
}

interface Bookkeeping {
	syncedAt: Date | null;
	lastError: string;
}

async function bookkeep(env: Env, serverId: string, b: Bookkeeping): Promise<void> {
	await env.db
		.insert(serverListSync)
		.values({ serverId, ...b, updatedAt: new Date() })
		.onConflictDoUpdate({ target: serverListSync.serverId, set: { ...b, updatedAt: new Date() } });
}

/** One transaction: state rows for what happened, the snapshot as it now stands, the sync row. */
async function record(
	env: Env,
	serverId: string,
	plan: SyncPlan,
	o: Outcome,
	now: Date,
	b: Bookkeeping
): Promise<void> {
	const applied = (r: { kind: Kind; steamId: string; listId: string }): StateUpsert => ({
		serverId,
		kind: r.kind,
		steamId: r.steamId,
		sourceListId: r.listId,
		state: 'applied',
		error: '',
		attemptedAt: now,
		updatedAt: now
	});
	const failed = (r: {
		kind: Kind;
		steamId: string;
		listId?: string;
		error: string;
	}): StateUpsert => ({
		serverId,
		kind: r.kind,
		steamId: r.steamId,
		sourceListId: r.listId ?? null,
		state: 'failed',
		error: r.error.slice(0, 300),
		attemptedAt: now,
		updatedAt: now
	});
	const upserts: StateUpsert[] = [
		...o.added.map(applied),
		...plan.confirms.map(applied),
		...o.failedAdds.map(failed),
		...o.failedRemoves.map((f) => failed({ ...f }))
	];
	const drops = [...o.removed, ...plan.deletes];
	await env.db.transaction(async (tx) => {
		for (const u of upserts) await upsertState(tx, u);
		for (const d of drops)
			await tx
				.delete(serverListState)
				.where(
					and(
						eq(serverListState.serverId, serverId),
						eq(serverListState.kind, d.kind),
						eq(serverListState.steamId, d.steamId)
					)
				);
		await tx
			.insert(serverListSync)
			.values({ serverId, ...b, updatedAt: now })
			.onConflictDoUpdate({ target: serverListSync.serverId, set: { ...b, updatedAt: now } });
	});
	await writeSnapshot(env, serverId, o.observed, now);
}

type StateUpsert = typeof serverListState.$inferInsert;

async function upsertState(db: DbOrTx, u: StateUpsert): Promise<void> {
	await db
		.insert(serverListState)
		.values(u)
		.onConflictDoUpdate({
			target: [serverListState.serverId, serverListState.kind, serverListState.steamId],
			set: {
				sourceListId: u.sourceListId,
				state: u.state,
				error: u.error,
				attemptedAt: u.attemptedAt,
				updatedAt: u.updatedAt
			}
		});
}

// ---- bans, enforced by the panel -----------------------------------------------------------------

/** How long a kick the game refused waits before it is tried again on the same player. */
const KICK_RETRY_MS = 30_000;

/**
 * A server's bans are the panel's alone: nothing is written to the game's ban list (some hosts
 * keep that list in a file an unban never leaves). The worker holds the bans its lists put on the
 * server (`bans`, from the last sync) and this removes any of those players found on it: one kick,
 * with the org's ban message as it reads now, on the connection the observation already holds.
 * Nothing at all happens on a server whose players are not banned.
 *
 * The worker's copy is as old as the last sync, so the entry is read again before the kick: one
 * taken off its list or run out since then removes nobody. That is one query, and only when such
 * a player is actually on the server.
 */
export async function kickBanned(
	env: Env,
	server: ServerRow,
	org: OrgRow,
	client: WardogsClient,
	present: string[],
	bans: Map<string, PanelBan>
): Promise<void> {
	for (const steamId of present) {
		const b = bans.get(steamId);
		if (!b) continue;
		const now = new Date();
		if (b.retryAt && b.retryAt > now.getTime()) continue;
		const entry = await liveEntry(env, server.id, b, now);
		if (!entry) {
			bans.delete(steamId);
			continue;
		}
		let error = '';
		try {
			await ACTIONS.kick.run(client, {
				steamId,
				reason: renderBanMessage(org.banMessage, { ...entry, entryId: entry.id })
			});
		} catch (err) {
			const f = failure(err);
			// gone between the look and the kick: that is what was wanted
			if (isGone(f)) continue;
			if (isUnreachable(f)) return;
			b.retryAt = now.getTime() + KICK_RETRY_MS;
			error = f.message;
		}
		await writeAudit(env, null, {
			actorName: 'ban list',
			server: { id: server.id, name: server.name },
			orgId: server.orgId,
			category: 'system',
			action: 'ban.enforce',
			target: steamId,
			outcome: error ? 'error' : 'ok',
			status: error ? 502 : 200,
			message: error ? `Could not remove a banned player: ${error}` : 'Banned player removed',
			detail: { banId: banUid(entry.id) }
		}).catch((err) => console.error('[warcon] ban.enforce audit', err));
	}
}

/** The entry behind a ban, if it is still live on a list the server subscribes to. */
async function liveEntry(env: Env, serverId: string, b: PanelBan, now: Date) {
	const [row] = await env.db
		.select({ entry: listEntries })
		.from(serverLists)
		.innerJoin(listEntries, eq(listEntries.listId, serverLists.listId))
		.where(
			and(
				eq(serverLists.serverId, serverId),
				eq(serverLists.listId, b.listId),
				eq(listEntries.steamId, b.steamId),
				isNull(listEntries.removedAt),
				or(isNull(listEntries.expiresAt), gt(listEntries.expiresAt, now))
			)
		)
		.limit(1);
	return row?.entry ?? null;
}

// ---- fan-out from the API ----------------------------------------------------------------------

/**
 * Pushes an org's lists to every one of its servers now. Waits up to FANOUT_WAIT_MS per server so
 * the caller's toast can be specific; anything slower carries on in the background and is
 * reported as still syncing.
 */
export async function fanOut(env: Env, org: OrgRow): Promise<ListSyncSummary> {
	await expireEntries(env).catch((err) => console.error('[warcon] list expiry', err));
	const rows = await env.db
		.select({ server: servers })
		.from(servers)
		.innerJoin(organizations, eq(organizations.id, servers.orgId))
		.where(eq(servers.orgId, org.id));
	const results = await Promise.all(
		rows.map(async ({ server }) => {
			const pending: SyncResult = {
				serverId: server.id,
				serverName: server.name,
				ok: false,
				added: 0,
				removed: 0,
				failed: 0,
				pending: true,
				error: ''
			};
			const work = reconcileServer(env, server, org, {
				reason: 'api',
				waitMs: FANOUT_WAIT_MS,
				lane: 0
			}).catch((err): SyncResult => ({
				...pending,
				pending: false,
				error: publicMessage(err, 'Sync failed.')
			}));
			const timer = new Promise<SyncResult>((r) => setTimeout(() => r(pending), FANOUT_WAIT_MS));
			return Promise.race([work, timer]);
		})
	);
	return { servers: results.map(summaryOf) };
}

/**
 * What an API answer says of a sync: where it landed, in counts. The rest of a SyncResult is the
 * worker's own (the server's lists, and the bans the worker enforces) and never leaves.
 */
export function summaryOf(r: SyncResult): ListSyncServer {
	const { serverId, serverName, ok, added, removed, failed, pending, error } = r;
	return { serverId, serverName, ok, added, removed, failed, pending, error };
}
