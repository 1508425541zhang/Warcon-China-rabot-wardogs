import { and, eq, sql } from 'drizzle-orm';
import { randomUUID } from 'node:crypto';
import type { Env } from '../env';
import type { DbOrTx } from '../db';
import { integrityRules, listEntries, outbox, siteSettings, type OutboxRow } from '../db/schema';
import { isOwner, withOwnedTransaction } from '../leadership';
import { memoryOf } from '../observe';
import { serverListOf } from '../lists';
import { vipFor } from '../qq/vip';
import {
	modelConfig,
	MODEL_CALIBRATION,
	validateModelResult,
	type ModelConfig
} from './model-http';
import { writeAudit } from '../audit';

type ModelRun = {
	id: string;
	org_id: string;
	server_id: string;
	steam_id: string;
	match_id: number;
	state: string;
	score: number | null;
	config_revision: string;
	created_at: Date;
	result: Record<string, unknown>;
	action: string | null;
	action_state: string | null;
	list_entry_id: string | null;
};
export function modelDecision(score: number) {
	if (!Number.isFinite(score) || score < 0) return null;
	return score >= MODEL_CALIBRATION.p99
		? 'QUARANTINE_24H'
		: score >= MODEL_CALIBRATION.p97
			? 'KICK'
			: null;
}

/** Fixed P97/P99 of the pinned model, with no expert votes or legacy-score dependency. */
export async function enforceModelRun(env: Env, id: string) {
	if (!isOwner()) return;
	const [candidate] = await env.db.execute<ModelRun>(
		sql`SELECT * FROM integrity_model_runs WHERE id=${id}`
	);
	if (!candidate || candidate.state !== 'READY' || candidate.action_state) return;
	const memory = memoryOf(candidate.server_id);
	const banList = memory ? await serverListOf(env, memory.server, 'ban') : null;
	const decision = await withOwnedTransaction(env, async (tx) => {
		await tx.execute(
			sql`SELECT pg_advisory_xact_lock(hashtextextended(${'integrityModel:' + candidate.org_id},0))`
		);
		await tx.execute(sql`SELECT pg_advisory_xact_lock(hashtextextended('qq-vip-settings',0))`);
		const [run] = await tx.execute<ModelRun>(
			sql`SELECT * FROM integrity_model_runs WHERE id=${id} FOR UPDATE`
		);
		if (!run || run.state !== 'READY' || run.action_state) return null;
		const skip = async (reason: string) => {
			await tx.execute(
				sql`UPDATE integrity_model_runs SET action_state='skipped',action_reason=${reason} WHERE id=${id}`
			);
			return null;
		};
		const [stored] = await tx
			.select()
			.from(siteSettings)
			.where(eq(siteSettings.key, 'integrityModel:' + run.org_id));
		const config = stored?.value as ModelConfig | undefined;
		const [rules] = await tx
			.select()
			.from(integrityRules)
			.where(eq(integrityRules.orgId, run.org_id))
			.for('update');
		if (
			!config?.developerEnabled ||
			!config.autoPunishEnabled ||
			config.revision !== run.config_revision ||
			rules?.assessmentMode !== 'model_only'
		)
			return skip('模型模式、自动处罚或配置版本已改变');
		const validated = validateModelResult(run.result, run.id);
		if (validated.status !== 'READY' || validated.score !== Number(run.score))
			return skip('模型结果无效');
		const action = modelDecision(validated.score!);
		if (!action) return skip('低于 P97');
		const m = memoryOf(run.server_id);
		if (
			!m?.ok ||
			!banList ||
			m.server.orgId !== run.org_id ||
			Date.now() - m.playersAt > 30000 ||
			Date.now() - m.statusAt > 30000 ||
			!m.players.some((p) => p.steamId === run.steam_id) ||
			Date.now() - new Date(run.created_at).getTime() > 300000
		)
			return skip('玩家离线、观测不健康或评分已过期');
		const [currentMatch] = await tx.execute(
			sql`SELECT id FROM matches WHERE server_id=${run.server_id} AND ended_at IS NULL ORDER BY started_at DESC LIMIT 1`
		);
		if (Number(currentMatch?.id) !== Number(run.match_id)) return skip('对局已改变');
		if ((await vipFor({ db: tx }, run.server_id, run.steam_id))?.whitelist)
			return skip('VIP 白名单免除自动风控处罚');
		const [recent] = await tx.execute(
			sql`SELECT count(*) AS n FROM integrity_model_runs WHERE org_id=${run.org_id} AND punished_at>now()-interval '1 hour'`
		);
		if (Number(recent.n) >= config.maxActionsPerHour) return skip('达到每小时自动处罚上限');
		const [cooldown] = await tx.execute(
			sql`SELECT 1 FROM integrity_model_runs WHERE server_id=${run.server_id} AND steam_id=${run.steam_id} AND punished_at>now()-(${config.cooldownSeconds}*interval '1 second') AND (action='QUARANTINE_24H' OR ${action}='KICK') LIMIT 1`
		);
		if (cooldown) return skip('玩家处罚冷却中');
		const [ban] = await tx.execute(
			sql`SELECT 1 FROM list_entries e JOIN server_lists sl ON sl.list_id=e.list_id JOIN lists l ON l.id=e.list_id WHERE sl.server_id=${run.server_id} AND l.kind='ban' AND e.steam_id=${run.steam_id} AND e.removed_at IS NULL AND (e.expires_at IS NULL OR e.expires_at>now()) LIMIT 1`
		);
		if (ban) return skip('已有有效封禁，保留原记录');
		const expires = action === 'QUARANTINE_24H' ? new Date(Date.now() + 86400000) : null;
		const entryId = expires ? randomUUID() : null;
		const reason = `模型 A测：异常分数达到 ${expires ? 'P99，临时隔离24小时' : 'P97，自动踢出'}。记录 ${id}，可联系管理员复核。`;
		if (entryId)
			await tx.insert(listEntries).values({
				id: entryId,
				listId: banList.id,
				steamId: run.steam_id,
				reason,
				expiresAt: expires,
				addedByName: 'Model A-test rule'
			});
		await tx.insert(outbox).values({
			serverId: run.server_id,
			triggerName: '模型 A测 P97/P99',
			triggerKind: 'model_integrity',
			action: 'kick',
			params: { steamId: run.steam_id, reason },
			target: run.steam_id,
			steamId: run.steam_id,
			okMessage: `模型 ${action}: ${run.steam_id}`,
			detail: { runId: id },
			dedupeKey: `model:${id}:kick`
		});
		await tx.execute(
			sql`UPDATE integrity_model_runs SET action=${action},action_state='pending',action_reason=${reason},punished_at=now(),expires_at=${expires},list_entry_id=${entryId} WHERE id=${id}`
		);
		return action;
	});
	if (decision)
		await writeAudit(env, null, {
			orgId: candidate.org_id,
			server: memory!.server,
			actorName: '模型 A测',
			category: 'trigger',
			action: 'model.' + decision,
			target: candidate.steam_id,
			outcome: 'ok',
			message: '模型自动处罚已入队',
			detail: {
				runId: id,
				decision,
				score: candidate.score,
				p97: MODEL_CALIBRATION.p97,
				p99: MODEL_CALIBRATION.p99
			}
		});
}

export async function modelDeliverySkipReason(
	env: Env,
	row: Pick<OutboxRow, 'triggerKind' | 'detail' | 'serverId' | 'steamId'>
) {
	if (row.triggerKind !== 'model_integrity') return null;
	const runId = (row.detail as { runId?: string })?.runId;
	const [run] = await env.db.execute<ModelRun>(
		sql`SELECT * FROM integrity_model_runs WHERE id=${runId || ''} AND server_id=${row.serverId} AND steam_id=${row.steamId}`
	);
	if (!run || !run.action) return '模型处罚记录缺失';
	const config = await modelConfig(env, run.org_id);
	const [mode] = await env.db.execute(
		sql`SELECT 1 FROM integrity_rules WHERE org_id=${run.org_id} AND assessment_mode='model_only'`
	);
	if (
		!config.developerEnabled ||
		!config.autoPunishEnabled ||
		config.revision !== run.config_revision ||
		!mode
	)
		return '模型自动处罚已停用或配置改变';
	if ((await vipFor(env, run.server_id, run.steam_id))?.whitelist)
		return 'VIP 白名单免除自动风控处罚';
	const m = memoryOf(run.server_id);
	if (!m?.ok || Date.now() - m.playersAt > 30000 || Date.now() - m.statusAt > 30000)
		return '服务器观测不健康';
	if (run.list_entry_id) {
		const [ban] = await env.db
			.select()
			.from(listEntries)
			.where(eq(listEntries.id, run.list_entry_id));
		if (!ban || ban.removedAt || !ban.expiresAt || ban.expiresAt <= new Date())
			return '模型隔离已撤销或到期';
	}
	return null;
}

export async function recordModelDelivery(
	db: DbOrTx,
	row: Pick<OutboxRow, 'triggerKind' | 'detail'>,
	state: string
) {
	if (row.triggerKind !== 'model_integrity') return;
	const id = (row.detail as { runId?: string })?.runId;
	if (id)
		await db.execute(sql`UPDATE integrity_model_runs SET action_state=${state} WHERE id=${id}`);
}
