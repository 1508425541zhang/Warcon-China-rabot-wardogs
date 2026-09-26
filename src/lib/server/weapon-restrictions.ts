import { and, eq, desc, asc, gte, isNull, lt, sql } from 'drizzle-orm';
import { createHash } from 'node:crypto';
import type { Env } from './env';
import {
	weaponRestrictionRules,
	weaponRestrictionEvents,
	kills,
	matches,
	type ServerRow
} from './db/schema';
import type { WardogsClient } from './rcon';
import type { Player, Status } from '$lib/types';
import { knownCauses, causeLabel } from '$lib/causes';
import { mapId } from '$lib/format';
import { restrictedCause, restrictionStage } from '$lib/weapon-restriction-policy';
import { ACTIONS } from './actions';
import { isOwner, withOwnedTransaction } from './leadership';
import { writeAudit } from './audit';
export async function weaponRestrictionView(env: Env, serverId: string) {
	const [[rule], events, observed] = await Promise.all([
		env.db
			.select()
			.from(weaponRestrictionRules)
			.where(eq(weaponRestrictionRules.serverId, serverId)),
		env.db
			.select()
			.from(weaponRestrictionEvents)
			.where(eq(weaponRestrictionEvents.serverId, serverId))
			.orderBy(desc(weaponRestrictionEvents.createdAt))
			.limit(50),
		env.db
			.selectDistinct({ cause: kills.cause })
			.from(kills)
			.where(and(eq(kills.serverId, serverId), gte(kills.ts, new Date(Date.now() - 30 * 86400000))))
			.limit(2001)
	]);
	const causes = [
		...new Set([
			...knownCauses().map((c) => c.cause),
			...observed.flatMap((c) => (c.cause ? [c.cause] : [])),
			...((rule?.causes ?? []) as string[])
		])
	].sort();
	return {
		rule: {
			enabled: rule?.enabled ?? false,
			causes: (rule?.causes ?? []) as string[],
			groups: (rule?.groups ?? []) as string[]
		},
		events: events.map((e) => ({
			...e,
			createdAt: e.createdAt.toISOString(),
			updatedAt: e.updatedAt.toISOString()
		})),
		catalogue: causes.map((cause) => ({ cause, label: causeLabel(cause) })),
		catalogueTruncated: observed.length > 2000
	};
}
export async function runWeaponRestrictions(
	env: Env,
	server: ServerRow,
	client: WardogsClient,
	input: { players: Player[]; status: Status; statusAt: number; boundary: boolean; now: Date }
) {
	const { now, status } = input;
	if (
		!isOwner() ||
		input.boundary ||
		Date.now() - input.statusAt > 30000 ||
		status.matchSeconds === null
	)
		return;
	const [rule] = await env.db
		.select()
		.from(weaponRestrictionRules)
		.where(eq(weaponRestrictionRules.serverId, server.id));
	if (!rule?.enabled) return;
	const [round] = await env.db
		.select()
		.from(matches)
		.where(and(eq(matches.serverId, server.id), isNull(matches.endedAt)))
		.orderBy(desc(matches.id))
		.limit(1);
	if (
		!round ||
		!round.map ||
		mapId(round.map) !== mapId(status.map) ||
		(status.scoreCap !== null && status.scores.some((s) => s.score >= status.scoreCap!))
	)
		return;
	const version = rule.updatedAt.toISOString();
	await env.db
		.update(weaponRestrictionEvents)
		.set({ state: 'error', reason: '执行中断，结果未知；不会自动补踢', updatedAt: now })
		.where(
			and(
				eq(weaponRestrictionEvents.serverId, server.id),
				eq(weaponRestrictionEvents.state, 'executing'),
				lt(weaponRestrictionEvents.updatedAt, new Date(now.getTime() - 120000))
			)
		);
	const rows = await env.db
		.select()
		.from(kills)
		.where(
			and(
				eq(kills.serverId, server.id),
				eq(kills.matchRow, round.id),
				gte(kills.ts, new Date(Math.max(rule.updatedAt.getTime(), now.getTime() - 60000)))
			)
		)
		.orderBy(asc(kills.ts), asc(kills.eventTime), asc(kills.eventId))
		.limit(2001);
	if (rows.length > 2000) return; // Backlog/overload is not permission to punish on partial observations.
	const online = new Map(input.players.map((p) => [p.steamId, p]));
	const acted = new Set<string>();
	for (const k of rows) {
		const steamId = k.killerSteamId;
		if (
			!steamId ||
			!/^\d{17}$/.test(steamId) ||
			k.suicide ||
			steamId === k.victimSteamId ||
			!online.has(steamId) ||
			acted.has(steamId) ||
			mapId(k.map) !== mapId(status.map) ||
			!restrictedCause(k.cause, rule.causes as string[], rule.groups as string[])
		)
			continue;
		const id = createHash('sha256')
			.update(JSON.stringify([server.id, version, k.instanceId, k.eventId]))
			.digest('hex');
		const claim = await withOwnedTransaction(env, async (tx) => {
			await tx.execute(
				sql`SELECT pg_advisory_xact_lock(hashtextextended(${server.id + ':weapon:' + steamId},0))`
			);
			const [active] = await tx
				.select()
				.from(weaponRestrictionRules)
				.where(eq(weaponRestrictionRules.serverId, server.id))
				.for('share');
			if (!active?.enabled || active.updatedAt.toISOString() !== version) return null;
			const prior = await tx
				.select()
				.from(weaponRestrictionEvents)
				.where(
					and(
						eq(weaponRestrictionEvents.serverId, server.id),
						eq(weaponRestrictionEvents.matchId, round.id),
						eq(weaponRestrictionEvents.ruleVersion, version),
						eq(weaponRestrictionEvents.steamId, steamId)
					)
				)
				.orderBy(desc(weaponRestrictionEvents.createdAt));
			if (prior.some((e) => e.id === id || e.state === 'executing')) return null;
			const warning = prior.find((e) => e.action === 'warn' && e.state === 'delivered');
			if (warning && k.ts.getTime() <= warning.updatedAt.getTime()) return null;
			const lastKick = prior.find((e) => e.action === 'kick' && e.state === 'delivered');
			const action = restrictionStage(
				k.eventTime,
				status.matchSeconds! + Math.max(0, now.getTime() - input.statusAt) / 1000,
				warning,
				lastKick?.clock ?? null
			);
			if (!action) return null;
			const [saved] = await tx
				.insert(weaponRestrictionEvents)
				.values({
					id,
					serverId: server.id,
					matchId: round.id,
					ruleVersion: version,
					steamId,
					playerName: online.get(steamId)!.name,
					cause: k.cause!,
					eventId: k.eventId,
					action,
					state: 'executing',
					reason: '等待执行',
					clock: status.matchSeconds! + Math.max(0, Date.now() - input.statusAt) / 1000,
					createdAt: now,
					updatedAt: now
				})
				.onConflictDoNothing()
				.returning();
			return saved ?? null;
		});
		if (!claim) continue;
		acted.add(steamId);
		let state = 'delivered',
			reason =
				claim.action === 'warn' ? '首次违规，警告已发送' : '警告后再次使用受限来源造成击杀，已踢出';
		try {
			const [active] = await env.db
				.select()
				.from(weaponRestrictionRules)
				.where(eq(weaponRestrictionRules.serverId, server.id));
			if (
				!isOwner() ||
				!active?.enabled ||
				active.updatedAt.toISOString() !== version ||
				Date.now() - input.statusAt > 30000
			) {
				state = 'skipped';
				reason = '规则已变更或数据已过期';
			} else if (claim.action === 'warn')
				await ACTIONS.whisper.run(client, {
					steamId,
					message: `武器限制警告：${causeLabel(k.cause).slice(0, 70)} 已被禁用。请立即更换；8秒后再用任何受限来源造成击杀将踢出。`
				});
			else
				await ACTIONS.kick.run(client, {
					steamId,
					reason: `武器限制：警告后再次使用 ${causeLabel(k.cause).slice(0, 90)} 造成击杀。`
				});
		} catch {
			state = 'error';
			reason = '游戏接口执行失败或结果未知；不会视为警告成功，不自动重试本事件';
		}
		await env.db
			.update(weaponRestrictionEvents)
			.set({
				state,
				reason,
				updatedAt: new Date(),
				...(state === 'delivered'
					? { clock: status.matchSeconds! + Math.max(0, Date.now() - input.statusAt) / 1000 }
					: {})
			})
			.where(eq(weaponRestrictionEvents.id, id));
		await writeAudit(env, null, {
			server,
			orgId: server.orgId,
			category: 'trigger',
			action: `weapon_restriction.${claim.action}`,
			target: steamId,
			outcome: state === 'delivered' ? 'ok' : 'error',
			detail: { cause: k.cause, eventId: k.eventId, reason }
		});
	}
}
