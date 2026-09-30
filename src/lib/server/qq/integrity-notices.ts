import { sql } from 'drizzle-orm';
import type { DbOrTx } from '../db';
import type { Env } from '../env';
import type { OutboxRow } from '../db/schema';
import { qqCredentials, qqPolicy } from './config';
import type { QqClient } from './protocol';
import { CALIBRATION_SHA, MODEL_CALIBRATION } from '../integrity/model-http';

const clean = (value: unknown, limit = 700) =>
	String(value ?? '')
		.replace(/[\x00-\x1f\x7f]/g, ' ')
		.slice(0, limit);
const number = (value: unknown) =>
	typeof value === 'number' && Number.isFinite(value) ? value : null;

/** Called with successful RCON finalization, in that same transaction; never infer cheating from reason text. */
export async function enqueueIntegrityNotice(db: DbOrTx, row: OutboxRow, at = new Date()) {
	if (
		row.action !== 'kick' ||
		!row.steamId ||
		!['integrity', 'model_integrity'].includes(row.triggerKind)
	)
		return;
	const policy = qqPolicy(row.serverId),
		credentials = qqCredentials();
	if (!policy || policy.antiCheatNotices === false || !credentials) return;
	const delivered = await db.execute(
		sql`SELECT id FROM outbox WHERE id=${row.id} AND server_id=${row.serverId} AND steam_id=${row.steamId} AND trigger_kind=${row.triggerKind} AND action='kick' AND state='delivered'`
	);
	if (!delivered.length) return;
	const detail = (row.detail ?? {}) as Record<string, any>;
	let source: string, percentile: string, reference: string, identity: string;
	if (row.triggerKind === 'model_integrity') {
		const [run] = await db.execute<{
			action: string;
			score: number;
			threshold: number;
			result: Record<string, any>;
		}>(
			sql`SELECT action,score,threshold,result FROM integrity_model_runs WHERE id=${String(detail.runId ?? '')} AND server_id=${row.serverId} AND steam_id=${row.steamId} AND state='READY' AND action_state='delivered'`
		);
		if (!run || !['KICK', 'QUARANTINE_24H'].includes(run.action))
			throw Error('Missing confirmed model kick identity');
		const calibration =
			detail.modelCalibration ??
			(run.result?.calibrationSha256 === CALIBRATION_SHA ? MODEL_CALIBRATION : null);
		const p95 = number(calibration?.p95),
			p97 = number(calibration?.p97),
			p98 = number(calibration?.p98),
			p99 = number(calibration?.p99);
		source = 'AI 自动决策（30 分钟时序模型）';
		percentile =
			run.action === 'QUARANTINE_24H'
				? 'P99 · 隔离24小时并踢出'
				: p98 === Number(run.threshold)
					? 'P98 · 自动踢出'
					: p97 === Number(run.threshold)
						? 'P97 · 自动踢出'
						: p95 === Number(run.threshold)
							? 'P95 · 自动踢出'
							: '自动踢出（历史阈值）';
		reference = `异常分数：${Number(run.score).toFixed(6)}\n踢出阈值：${Number(run.threshold).toFixed(6)}${p95 !== null ? '\n参考 P95：' + p95.toFixed(6) : ''}${p97 !== null ? ' · P97：' + p97.toFixed(6) : ''}${p98 !== null ? ' · P98：' + p98.toFixed(6) : ''}${p99 !== null ? ' · P99：' + p99.toFixed(6) : ''}\n异常分数不是作弊概率。`;
		identity = '模型记录：' + clean(detail.runId, 100);
	} else {
		const [action] = await db.execute<{ source: string }>(
			sql`SELECT source FROM integrity_actions WHERE id=${String(detail.actionId ?? '')} AND case_id=${String(detail.caseId ?? '')} AND server_id=${row.serverId} AND steam_id=${row.steamId} AND delivery_state='delivered' AND action IN ('KICK','QUARANTINE_24H','QUARANTINE_7D')`
		);
		if (!action || !['RULE', 'REVIEW'].includes(action.source))
			throw Error('Missing confirmed anti-cheat action identity');
		source =
			action.source === 'REVIEW'
				? '管理员人工审核' + (detail.reviewerName ? ' · ' + clean(detail.reviewerName, 80) : '')
				: '反作弊规则自动处置';
		percentile = '不适用（本次不是模型百分位处罚）';
		reference =
			action.source === 'REVIEW' && detail.reviewReason
				? '人工审核说明：' + clean(detail.reviewReason)
				: '';
		identity = '案件：' + clean(detail.caseId, 100);
	}
	const [server] = await db.execute<{ name: string }>(
		sql`SELECT name FROM servers WHERE id=${row.serverId}`
	);
	const [player] = await db.execute<{ name: string }>(
		sql`SELECT name FROM player_sessions WHERE server_id=${row.serverId} AND steam_id=${row.steamId} ORDER BY last_seen DESC LIMIT 1`
	);
	const content = [
		'【反作弊踢出通知】',
		'服务器：' + clean(server?.name ?? row.serverId, 100),
		'玩家：' + clean(detail.playerName ?? player?.name ?? '未知昵称', 100),
		'SteamID64：' + row.steamId,
		'操作来源：' + source,
		'处罚档位：' + percentile,
		'踢出理由：' + clean((row.params as any)?.reason ?? row.okMessage),
		reference,
		identity,
		'时间：' +
			at.toLocaleString('zh-CN', { timeZone: 'Asia/Hong_Kong', hour12: false }) +
			'（UTC+8）'
	]
		.filter(Boolean)
		.join('\n');
	for (const group of policy.groups)
		await db.execute(
			sql`INSERT INTO qq_integrity_notifications(outbox_id,server_id,group_id,self_id,content,created_at) VALUES(${row.id},${row.serverId},${group},${credentials.selfId},${content},${at}) ON CONFLICT DO NOTHING`
		);
}

/** Caller holds the existing cross-process QQ sender lock. Unknown sends are never replayed. */
export async function processIntegrityNotice(env: Env, client: QqClient, selfId: string) {
	await env.db.execute(
		sql`UPDATE qq_integrity_notifications SET state='unknown',outcome='发送中断，结果未知，不自动重发' WHERE state='sending'`
	);
	const [notice] = await env.db.execute<{
		outbox_id: number;
		server_id: string;
		group_id: string;
		self_id: string;
		content: string;
		created_at: Date;
	}>(
		sql`SELECT * FROM qq_integrity_notifications WHERE state='pending' ORDER BY created_at,outbox_id,group_id LIMIT 1`
	);
	if (!notice) return;
	const policy = qqPolicy(notice.server_id);
	if (
		notice.self_id !== selfId ||
		!policy ||
		policy.antiCheatNotices === false ||
		!policy.groups.includes(notice.group_id) ||
		Date.now() - new Date(notice.created_at).getTime() > 86400000
	) {
		await env.db.execute(
			sql`UPDATE qq_integrity_notifications SET state='skipped',outcome='通知过期或群、账号、通知授权已改变',finished_at=now() WHERE outbox_id=${notice.outbox_id} AND group_id=${notice.group_id}`
		);
		return;
	}
	// A disconnected bot leaves the notice pending. This probe cannot send a group message.
	if (!(await client.loggedIn(selfId))) return;
	const claimed = await env.db.execute(
		sql`UPDATE qq_integrity_notifications SET state='sending' WHERE outbox_id=${notice.outbox_id} AND group_id=${notice.group_id} AND state='pending' RETURNING outbox_id`
	);
	if (!claimed.length) return;
	let state = 'delivered';
	try {
		await client.reply(notice.group_id, 'integrity:' + notice.outbox_id, notice.content);
	} catch {
		state = 'unknown';
	}
	await env.db.execute(
		sql`UPDATE qq_integrity_notifications SET state=${state},outcome=${state === 'delivered' ? 'OneBot确认发送' : '发送结果未知，不自动重发'},finished_at=now() WHERE outbox_id=${notice.outbox_id} AND group_id=${notice.group_id}`
	);
}
