import { eq, inArray, sql } from 'drizzle-orm';
import type { Env } from '../env';
import { servers, siteSettings } from '../db/schema';
import {
	HISTORY_RETENTION_DEFAULTS,
	validHistoryPolicy,
	type HistoryRetentionPolicy
} from '$lib/integrity-retention';
import { isOwner, withOwnedTransaction } from '../leadership';
import { ApiError } from '../http';
import { writeAudit } from '../audit';
import type { SessionUser } from '../access';
import { historyIndex } from './action-history';

const keyFor = (serverId: string) => 'integrityRetention:' + serverId;
export async function historyPolicy(env: Env, serverId: string) {
	const values = await env.db
		.select()
		.from(siteSettings)
		.where(inArray(siteSettings.key, [keyFor(serverId), 'integrityRetentionLast:' + serverId]));
	const row = values.find((row) => row.key === keyFor(serverId));
	const last = values.find((row) => row.key === 'integrityRetentionLast:' + serverId)?.value as
		| { at?: string; actions?: number; modelRuns?: number; short?: number; cases?: number }
		| undefined;
	return {
		policy: validHistoryPolicy(row?.value) ? row.value : { ...HISTORY_RETENTION_DEFAULTS },
		revision: row?.updatedAt.toISOString() ?? '',
		lastCleanup:
			typeof last?.at === 'string'
				? {
						at: last.at,
						removed:
							(last.actions ?? 0) + (last.modelRuns ?? 0) + (last.short ?? 0) + (last.cases ?? 0)
					}
				: null
	};
}
export async function saveHistoryPolicy(
	env: Env,
	actor: SessionUser,
	serverId: string,
	policy: unknown,
	revision: string
) {
	if (!validHistoryPolicy(policy))
		throw new ApiError(
			400,
			'每页支持 10／20／50／100 条；保留上限为 100–100000 条，时间为 7–3650 天。'
		);
	const [server] = await env.db.select().from(servers).where(eq(servers.id, serverId));
	if (!server) throw new ApiError(404, 'Server not found.');
	const result = await env.db.transaction(async (tx) => {
		await tx.execute(sql`SELECT pg_advisory_xact_lock(hashtextextended(${keyFor(serverId)},0))`);
		const [before] = await tx
			.select()
			.from(siteSettings)
			.where(eq(siteSettings.key, keyFor(serverId)))
			.for('update');
		if ((before?.updatedAt.toISOString() ?? '') !== revision)
			throw new ApiError(409, '保留设置已更新，请刷新后重试。');
		const updatedAt = new Date(Math.max(Date.now(), (before?.updatedAt.getTime() ?? 0) + 1));
		await tx
			.insert(siteSettings)
			.values({ key: keyFor(serverId), value: policy, updatedAt })
			.onConflictDoUpdate({ target: siteSettings.key, set: { value: policy, updatedAt } });
		return { policy, revision: updatedAt.toISOString() };
	});
	await writeAudit(env, null, {
		orgId: server.orgId,
		server,
		actor: { id: actor.id, username: actor.username },
		category: 'server',
		action: 'integrity.historyRetention',
		target: serverId,
		outcome: 'ok',
		message: '处罚历史分页与自动保留设置已更新',
		detail: policy
	});
	return result;
}

/** One bounded, owned transaction. Current enforcement state and seven-day escalation evidence survive. */
export async function pruneIntegrityHistory(env: Env, serverId: string) {
	if (!isOwner()) return null;
	return withOwnedTransaction(env, async (tx) => {
		await tx.execute(sql`SELECT pg_advisory_xact_lock(hashtextextended(${keyFor(serverId)},0))`);
		const [setting] = await tx
			.select()
			.from(siteSettings)
			.where(eq(siteSettings.key, keyFor(serverId)))
			.for('update');
		if (!validHistoryPolicy(setting?.value) || !setting.value.autoDeleteEnabled) return null;
		const policy = setting.value;
		const prunable = sql`(
		 (kind='action' AND EXISTS(SELECT 1 FROM integrity_actions a WHERE a.id=h.raw_id
		  AND coalesce(a.delivery_state,CASE WHEN a.effective_at IS NOT NULL THEN 'delivered' ELSE 'unknown' END) IN ('delivered','failed','skipped')
		  AND (a.expires_at IS NULL OR a.expires_at<=now())
		  AND NOT EXISTS(SELECT 1 FROM list_entries e WHERE e.id=a.list_entry_id AND e.removed_at IS NULL AND (e.expires_at IS NULL OR e.expires_at>now()))))
		 OR (kind='long' AND EXISTS(SELECT 1 FROM integrity_model_runs r WHERE r.id=h.raw_id
		  AND r.action_state IN ('delivered','failed','skipped') AND (r.expires_at IS NULL OR r.expires_at<=now())
		  AND NOT EXISTS(SELECT 1 FROM list_entries e WHERE e.id=r.list_entry_id AND e.removed_at IS NULL AND (e.expires_at IS NULL OR e.expires_at>now()))))
		 OR (kind='short' AND EXISTS(SELECT 1 FROM site_settings s WHERE s.key=h.raw_id
		  AND coalesce(s.value->>'state','warning') IN ('warning','delivered','failed','skipped')
		  AND NOT EXISTS(SELECT 1 FROM matches m WHERE m.server_id=${serverId} AND m.ended_at IS NULL AND m.id::text=s.value->'scope'->>2)))
		 )
		 AND NOT EXISTS(SELECT 1 FROM outbox o WHERE o.server_id=${serverId}
		 AND ((kind='action' AND o.trigger_kind='integrity' AND o.detail->>'actionId'=h.raw_id)
		 OR (kind='long' AND o.trigger_kind='model_integrity' AND o.detail->>'runId'=h.raw_id))
		 AND (o.state NOT IN ('delivered','failed','skipped') OR EXISTS(SELECT 1 FROM qq_integrity_notifications n WHERE n.outbox_id=o.id AND n.state NOT IN ('delivered','skipped'))))`;
		// Rank all sources together. Active/uncertain rows may exceed the quota and are never discarded.
		await tx.execute(sql`CREATE TEMP TABLE history_retention_targets ON COMMIT DROP AS
		 WITH history AS (${historyIndex(serverId)}), ranked AS (
		 SELECT *,row_number() OVER(ORDER BY created_at DESC,id DESC) AS ordinal FROM history
		 ) SELECT h.* FROM ranked h
		 WHERE (ordinal>${policy.maxRecords} OR created_at<now()-(${policy.maxAgeDays}*interval '1 day'))
		 AND created_at<now()-interval '7 days'
		 AND ${prunable}
		 ORDER BY created_at,id LIMIT 500`);
		// Normal model results also consume space, despite not appearing in punishment history.
		await tx.execute(sql`CREATE TEMP TABLE history_retention_runs ON COMMIT DROP AS
		 WITH ranked AS (SELECT id,created_at,state,action_state,row_number() OVER(ORDER BY created_at DESC,id DESC) AS ordinal
		 FROM integrity_model_runs WHERE server_id=${serverId} AND action IS NULL AND punished_at IS NULL)
		 SELECT id FROM ranked WHERE (ordinal>${policy.maxRecords} OR created_at<now()-(${policy.maxAgeDays}*interval '1 day'))
		 AND created_at<now()-interval '7 days' AND (state IN ('ERROR','superseded') OR (state='READY' AND action_state='skipped'))
		 ORDER BY created_at,id LIMIT 500`);
		// Lock candidates and linked enforcement state, then repeat the eligibility check.
		await tx.execute(
			sql`SELECT c.id FROM integrity_cases c WHERE c.id IN (SELECT a.case_id FROM integrity_actions a JOIN history_retention_targets h ON h.kind='action' AND h.raw_id=a.id) ORDER BY c.id FOR UPDATE`
		);
		await tx.execute(
			sql`SELECT a.id FROM integrity_actions a JOIN history_retention_targets h ON h.kind='action' AND h.raw_id=a.id ORDER BY a.id FOR UPDATE OF a`
		);
		await tx.execute(
			sql`SELECT r.id FROM integrity_model_runs r WHERE r.id IN (SELECT raw_id FROM history_retention_targets WHERE kind='long' UNION SELECT id FROM history_retention_runs) ORDER BY r.id FOR UPDATE`
		);
		await tx.execute(
			sql`SELECT s.key FROM site_settings s JOIN history_retention_targets h ON h.kind='short' AND h.raw_id=s.key ORDER BY s.key FOR UPDATE OF s`
		);
		await tx.execute(
			sql`SELECT e.id FROM list_entries e WHERE e.id IN (SELECT a.list_entry_id FROM integrity_actions a JOIN history_retention_targets h ON h.kind='action' AND h.raw_id=a.id UNION SELECT r.list_entry_id FROM integrity_model_runs r JOIN history_retention_targets h ON h.kind='long' AND h.raw_id=r.id) ORDER BY e.id FOR SHARE`
		);
		await tx.execute(
			sql`SELECT o.id FROM outbox o JOIN history_retention_targets h ON (h.kind='action' AND o.trigger_kind='integrity' AND o.detail->>'actionId'=h.raw_id) OR (h.kind='long' AND o.trigger_kind='model_integrity' AND o.detail->>'runId'=h.raw_id) WHERE o.server_id=${serverId} ORDER BY o.id FOR UPDATE OF o`
		);
		await tx.execute(sql`DELETE FROM history_retention_targets h WHERE NOT (${prunable})`);
		await tx.execute(
			sql`DELETE FROM history_retention_runs t USING integrity_model_runs r WHERE r.id=t.id AND (r.action IS NOT NULL OR r.punished_at IS NOT NULL OR NOT coalesce((r.state IN ('ERROR','superseded') OR (r.state='READY' AND r.action_state='skipped')),false))`
		);
		await tx.execute(sql`CREATE TEMP TABLE history_retention_cases ON COMMIT DROP AS SELECT DISTINCT a.case_id AS id
		 FROM integrity_actions a JOIN history_retention_targets h ON h.kind='action' AND h.raw_id=a.id`);
		await tx.execute(sql`CREATE TEMP TABLE history_retention_outbox ON COMMIT DROP AS SELECT o.id FROM outbox o
		 JOIN history_retention_targets h ON (h.kind='action' AND o.trigger_kind='integrity' AND o.detail->>'actionId'=h.raw_id)
		 OR (h.kind='long' AND o.trigger_kind='model_integrity' AND o.detail->>'runId'=h.raw_id)
		 WHERE o.server_id=${serverId}`);
		await tx.execute(sql`DELETE FROM audit_log a WHERE a.server_id=${serverId} AND a.ts<now()-interval '7 days'
		 AND (a.detail->>'actionId' IN (SELECT raw_id FROM history_retention_targets WHERE kind='action')
		 OR a.detail->>'runId' IN (SELECT raw_id FROM history_retention_targets WHERE kind='long' UNION SELECT id FROM history_retention_runs)
		 OR a.detail->>'outboxId' IN (SELECT id::text FROM history_retention_outbox)
		 OR (a.action='shortRisk.kick' AND EXISTS(SELECT 1 FROM site_settings s JOIN history_retention_targets h ON h.kind='short' AND h.raw_id=s.key WHERE a.detail->>'steamId'=s.value->>'steamId' AND a.detail->'scope'=s.value->'scope')))`);
		await tx.execute(sql`DELETE FROM outbox WHERE id IN (SELECT id FROM history_retention_outbox)`);
		const actions = await tx.execute(
			sql`DELETE FROM integrity_actions WHERE id IN (SELECT raw_id FROM history_retention_targets WHERE kind='action') RETURNING id`
		);
		const runs = await tx.execute(
			sql`DELETE FROM integrity_model_runs WHERE id IN (SELECT raw_id FROM history_retention_targets WHERE kind='long' UNION SELECT id FROM history_retention_runs) RETURNING id`
		);
		const short = await tx.execute(
			sql`DELETE FROM site_settings WHERE key IN (SELECT raw_id FROM history_retention_targets WHERE kind='short') RETURNING key`
		);
		// Only concluded cases whose last punishment was pruned; open investigations remain intact.
		await tx.execute(
			sql`DELETE FROM history_retention_cases t WHERE NOT EXISTS(SELECT 1 FROM integrity_cases c WHERE c.id=t.id AND (c.reviewed_at IS NOT NULL OR c.status<>'OPEN') AND c.created_at<now()-interval '7 days') OR EXISTS(SELECT 1 FROM integrity_actions a WHERE a.case_id=t.id)`
		);
		await tx.execute(
			sql`DELETE FROM integrity_labels WHERE case_id IN (SELECT id FROM history_retention_cases)`
		);
		await tx.execute(
			sql`UPDATE integrity_reports SET case_id=NULL WHERE case_id IN (SELECT id FROM history_retention_cases)`
		);
		await tx.execute(
			sql`DELETE FROM audit_log WHERE server_id=${serverId} AND ts<now()-interval '7 days' AND detail->>'caseId' IN (SELECT id FROM history_retention_cases)`
		);
		const cases = await tx.execute(
			sql`DELETE FROM integrity_cases WHERE id IN (SELECT id FROM history_retention_cases) RETURNING id`
		);
		const result = {
			actions: actions.length,
			modelRuns: runs.length,
			short: short.length,
			cases: cases.length
		};
		const at = new Date();
		await tx
			.insert(siteSettings)
			.values({
				key: 'integrityRetentionLast:' + serverId,
				value: { ...result, at: at.toISOString() },
				updatedAt: at
			})
			.onConflictDoUpdate({
				target: siteSettings.key,
				set: { value: { ...result, at: at.toISOString() }, updatedAt: at }
			});
		return result;
	});
}
let timer: ReturnType<typeof setInterval> | undefined;
let initial: ReturnType<typeof setTimeout> | undefined;
let running: Promise<void> | undefined;
export function startHistoryRetention(env: Env) {
	if (timer) return;
	const pass = () => {
		if (!isOwner() || running) return;
		running = (async () => {
			const configured = await env.db
				.select({ serverId: servers.id, value: siteSettings.value })
				.from(servers)
				.innerJoin(siteSettings, sql`${siteSettings.key}='integrityRetention:'||${servers.id}`);
			for (const row of configured)
				if (validHistoryPolicy(row.value) && row.value.autoDeleteEnabled)
					await pruneIntegrityHistory(env, row.serverId);
		})()
			.catch((error) =>
				console.error('[integrity-retention]', error instanceof Error ? error.name : 'error')
			)
			.finally(() => {
				running = undefined;
			});
	};
	initial = setTimeout(pass, 30000);
	timer = setInterval(pass, 3600000);
}
export async function stopHistoryRetention() {
	if (timer) clearInterval(timer);
	if (initial) clearTimeout(initial);
	timer = undefined;
	initial = undefined;
	await running;
}
