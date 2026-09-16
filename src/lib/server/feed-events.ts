// What happens in the worker once the kill feed has written a batch: the kills go out on the
// event bus (the SSE route fans them to browsers; in the split roles every web process gets them
// through the relay stream), and the team-kill rules get their turn. Their intents go through
// the same outbox as every other trigger, so delivery, audit and the Discord mirror are shared.
import { and, eq, gte, isNull, sql } from 'drizzle-orm';
import type { Env } from './env';
import { emit } from './events';
import { kills, playerSessions } from './db/schema';
import { enabledTriggers, renderTemplate, teamKillStage, type Evaluation } from './triggers';
import type { TeamKillConfig } from './trigger-rules';
import { applyTriggerUpdates, enqueueIntents, wakeDelivery } from './outbox';
import { LostOwnership, withOwnedTransaction } from './leadership';
import { memoryOf } from './observe';
import { publicMessage } from './http';
import type { KillView } from '$lib/types';

export async function onKillsIngested(
	env: Env,
	serverId: string,
	kills: KillView[]
): Promise<void> {
	if (!kills.length) return;
	emit({ type: 'kills', serverId, kills });
	const teamKills = kills.filter((k) => k.teamKill && k.killer);
	if (!teamKills.length) return;
	try {
		await actOnTeamKills(env, serverId, teamKills);
	} catch (err) {
		if (err instanceof LostOwnership) return;
		console.warn(`[warcon] team-kill rules on ${serverId}:`, publicMessage(err));
	}
}

/** How many team kills this player has in their current session on the server, including these. */
async function teamKillsThisSession(env: Env, serverId: string, steamId: string): Promise<number> {
	const [open] = await env.db
		.select({ joinedAt: playerSessions.joinedAt })
		.from(playerSessions)
		.where(
			and(
				eq(playerSessions.serverId, serverId),
				eq(playerSessions.steamId, steamId),
				isNull(playerSessions.leftAt)
			)
		)
		.orderBy(sql`${playerSessions.id} DESC`)
		.limit(1);
	// No open session known (the worker just restarted, or the join was not trusted): the last hour.
	const since = open?.joinedAt ?? new Date(Date.now() - 3600_000);
	const [row] = await env.db
		.select({ n: sql<number>`COUNT(*)` })
		.from(kills)
		.where(
			and(
				eq(kills.serverId, serverId),
				eq(kills.killerSteamId, steamId),
				eq(kills.teamKill, true),
				gte(kills.ts, since)
			)
		);
	return Number(row?.n ?? 0);
}

async function actOnTeamKills(env: Env, serverId: string, teamKills: KillView[]): Promise<void> {
	const rows = (await enabledTriggers(env, serverId)).filter((r) => r.kind === 'team_kill');
	if (!rows.length) return;
	const m = memoryOf(serverId);
	const serverName = m?.status?.serverName || m?.server.name || '';
	const out: Evaluation = { intents: [], updates: [] };
	// One count per killer for the batch; the last of their kills in it is the one acted on.
	const byKiller = new Map<string, KillView>();
	for (const k of teamKills) byKiller.set(k.killer!.steamId, k);
	for (const [steamId, k] of byKiller) {
		const count = await teamKillsThisSession(env, serverId, steamId);
		const v = {
			name: k.killer!.name,
			victim: k.victim.name,
			count,
			server: serverName,
			map: k.map
		};
		for (const row of rows) {
			const cfg = row.config as TeamKillConfig;
			const stage = teamKillStage(cfg, count);
			if (!stage) continue;
			const kick = stage === 'kick';
			const text = renderTemplate(kick ? cfg.kickReason : cfg.warnMessage, v);
			out.intents.push({
				trigger: row,
				action: kick ? 'kick' : 'whisper',
				params: kick ? { steamId, reason: text } : { steamId, message: text },
				target: steamId,
				okMessage: kick
					? `Kicked ${v.name} after ${count} team kill${count === 1 ? '' : 's'}`
					: `Whispered ${v.name} (${count} team kill${count === 1 ? '' : 's'})`,
				detail: { name: v.name, victim: v.victim, count, eventId: k.eventId },
				steamId,
				dedupeKey: [row.id, steamId, k.eventId].join(':')
			});
			out.updates.push({
				id: row.id,
				lastFiredAt: new Date(),
				lastResult: kick ? `Kicking ${v.name} (${count})` : `Whispering ${v.name} (${count})`
			});
		}
	}
	if (!out.intents.length) return;
	let queued = 0;
	await withOwnedTransaction(env, async (tx) => {
		queued = await enqueueIntents(tx, serverId, out.intents);
		await applyTriggerUpdates(tx, out.updates);
	});
	if (queued) wakeDelivery();
}
