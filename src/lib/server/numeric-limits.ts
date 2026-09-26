import { and, desc, eq, gte, isNull, sql } from 'drizzle-orm';
import {
	defaultNumericLimits,
	numericBreaches,
	validNumericLimits,
	type NumericLimits,
	type LimitObservation
} from '$lib/numeric-limit-policy';
import {
	numericLimitRules,
	numericLimitEvents,
	matches,
	playerProgressSamples,
	playerSessions,
	type ServerRow
} from './db/schema';
import type { Env } from './env';
import type { Player, Status } from '$lib/types';
import type { WardogsClient } from './rcon';
import { isOwner, withOwnedTransaction } from './leadership';
import { ACTIONS } from './actions';
import { mapId } from '$lib/format';
import { writeAudit } from './audit';

export async function numericLimitView(env: Env, serverId: string) {
	const [[rule], events] = await Promise.all([
		env.db.select().from(numericLimitRules).where(eq(numericLimitRules.serverId, serverId)),
		env.db
			.select()
			.from(numericLimitEvents)
			.where(eq(numericLimitEvents.serverId, serverId))
			.orderBy(desc(numericLimitEvents.createdAt))
			.limit(50)
	]);
	return {
		config: (rule?.config ?? defaultNumericLimits) as NumericLimits,
		events: events.map((e) => ({
			...e,
			createdAt: e.createdAt.toISOString(),
			updatedAt: e.updatedAt.toISOString()
		}))
	};
}

/** Invoked in the observer's per-server dispatcher lane. Never requires Kill Feed. */
export async function runNumericLimits(
	env: Env,
	server: ServerRow,
	client: WardogsClient,
	input: {
		players: Player[];
		status: Status;
		statusAt: number;
		trusted: boolean;
		boundary: boolean;
		now: Date;
	}
) {
	if (!isOwner() || !input.trusted || input.boundary || Date.now() - input.statusAt > 30000) return;
	const [rule] = await env.db
		.select()
		.from(numericLimitRules)
		.where(eq(numericLimitRules.serverId, server.id));
	if (!rule || !validNumericLimits(rule.config) || !rule.config.enabled) return;
	const config = rule.config,
		version = rule.updatedAt.toISOString();
	const [round] = await env.db
		.select()
		.from(matches)
		.where(and(eq(matches.serverId, server.id), isNull(matches.endedAt)))
		.orderBy(desc(matches.id))
		.limit(1);
	if (
		!round ||
		mapId(round.map ?? '') !== mapId(input.status.map) ||
		(input.status.scoreCap !== null &&
			input.status.scores.some((s) => s.score >= input.status.scoreCap!))
	)
		return;
	const start = new Date(input.now.getTime() - (config.windowSeconds + 60) * 1000);
	const [samples, sessions] = await Promise.all([
		env.db
			.select()
			.from(playerProgressSamples)
			.where(
				and(
					eq(playerProgressSamples.serverId, server.id),
					eq(playerProgressSamples.matchId, round.id),
					gte(playerProgressSamples.observedAt, start)
				)
			)
			.orderBy(playerProgressSamples.observedAt),
		env.db
			.select()
			.from(playerSessions)
			.where(and(eq(playerSessions.serverId, server.id), isNull(playerSessions.leftAt)))
	]);
	for (const player of input.players) {
		const joined = sessions.find((s) => s.steamId === player.steamId)?.joinedAt;
		if (!joined || input.now.getTime() - joined.getTime() < config.windowSeconds * 1000) continue;
		const eligible = samples.filter(
			(s) => s.observedAt >= joined && s.observedAt >= rule.updatedAt && s.observedAt < input.now
		);
		const cutoff = input.now.getTime() - config.windowSeconds * 1000;
		const first = eligible.findLastIndex((s) => s.observedAt.getTime() <= cutoff);
		if (first < 0) continue;
		const points: LimitObservation[] = eligible.slice(first).map((s) => {
			const p = (s.players as Player[]).find((p) => p.steamId === player.steamId);
			return {
				at: s.observedAt.getTime(),
				kills: p?.kills ?? NaN,
				deaths: p?.deaths ?? NaN,
				cash: p?.cash ?? NaN
			};
		});
		points.push({
			at: input.now.getTime(),
			kills: player.kills,
			deaths: player.deaths,
			cash: player.cash
		});
		const breaches = numericBreaches(config, points);
		if (!breaches.length) continue;
		const claim = await withOwnedTransaction(env, async (tx) => {
			await tx.execute(
				sql`SELECT pg_advisory_xact_lock(hashtextextended(${server.id + ':numeric:' + player.steamId},0))`
			);
			const [active] = await tx
				.select()
				.from(numericLimitRules)
				.where(eq(numericLimitRules.serverId, server.id))
				.for('share');
			if (
				!active ||
				active.updatedAt.toISOString() !== version ||
				!(active.config as NumericLimits).enabled
			)
				return null;
			const prior = await tx
				.select()
				.from(numericLimitEvents)
				.where(
					and(
						eq(numericLimitEvents.serverId, server.id),
						eq(numericLimitEvents.matchId, round.id),
						eq(numericLimitEvents.steamId, player.steamId),
						eq(numericLimitEvents.ruleVersion, version)
					)
				)
				.orderBy(desc(numericLimitEvents.createdAt));
			// Ambiguous sends require human inspection; never compound an unconfirmed warning with a kick.
			if (prior.some((p) => p.state === 'sending' || p.state === 'unknown' || p.action === 'kick'))
				return null;
			const warning = prior.find((p) => p.action === 'warn' && p.state === 'delivered');
			if (
				warning &&
				(points[0].at <= warning.updatedAt.getTime() ||
					(player.kills <= Number((warning.evidence as { kills: number }).kills) &&
						breaches.every((b) => b.metric === 'kd')))
			)
				return null;
			const action = warning ? 'kick' : 'warn';
			const [event] = await tx
				.insert(numericLimitEvents)
				.values({
					id: crypto.randomUUID(),
					serverId: server.id,
					matchId: round.id,
					steamId: player.steamId,
					ruleVersion: version,
					action,
					state: 'sending',
					evidence: {
						name: player.name,
						kills: player.kills,
						from: points[0].at,
						to: input.now.getTime(),
						breaches
					},
					createdAt: input.now
				})
				.returning();
			return event;
		});
		if (!claim) continue;
		let state = 'unknown';
		try {
			const [active] = await env.db
				.select()
				.from(numericLimitRules)
				.where(eq(numericLimitRules.serverId, server.id));
			if (
				!isOwner() ||
				!active ||
				active.updatedAt.toISOString() !== version ||
				!(active.config as NumericLimits).enabled ||
				Date.now() - input.statusAt > 30000
			)
				state = 'skipped';
			else {
				const label = breaches
					.map(
						(b) =>
							`${b.metric === 'cash' ? '金钱净增长/分钟' : b.metric.toUpperCase()} ${b.value.toFixed(2)}>${b.limit}`
					)
					.join('；');
				if (claim.action === 'warn')
					await ACTIONS.whisper.run(client, {
						steamId: player.steamId,
						message: `数值限制警告：${label}。再经过完整${config.windowSeconds}秒窗口仍超限将踢出。`
					});
				else
					await ACTIONS.kick.run(client, {
						steamId: player.steamId,
						reason: `数值限制：警告后再次超限。${label}`
					});
				state = 'delivered';
			}
		} catch {
			state = 'unknown';
		}
		await env.db
			.update(numericLimitEvents)
			.set({ state, updatedAt: new Date() })
			.where(eq(numericLimitEvents.id, claim.id));
		await writeAudit(env, null, {
			server,
			orgId: server.orgId,
			category: 'trigger',
			action: `numeric_limit.${claim.action}`,
			target: player.steamId,
			outcome: state === 'delivered' ? 'ok' : 'error',
			detail: { eventId: claim.id, breaches, state }
		});
	}
}
