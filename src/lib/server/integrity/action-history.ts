import { sql } from 'drizzle-orm';
import type { Env } from '../env';
import { MODEL_CALIBRATION } from './model-http';
import { SHORT_KICK, SHORT_WARNING } from '$lib/short-risk-policy';
export type ActionFilter = 'all' | 'automatic' | 'manual';
export function historyIndex(serverId: string) {
	return sql`SELECT a.id,a.id AS raw_id,'action'::text AS kind,a.source,a.created_at FROM integrity_actions a WHERE a.server_id=${serverId}
	 UNION ALL SELECT 'long:'||r.id,r.id,'long','LONG_MODEL',r.punished_at FROM integrity_model_runs r WHERE r.server_id=${serverId} AND r.action IS NOT NULL AND r.punished_at IS NOT NULL
	 UNION ALL SELECT 'short:'||s.key,s.key,'short','SHORT_MODEL',coalesce((s.value->>'attemptedAt')::timestamptz,(s.value->>'createdAt')::timestamptz,s.updated_at) FROM site_settings s WHERE s.key LIKE 'shortRisk:%' AND s.value->>'serverId'=${serverId}`;
}
export async function integrityActionPage(
	env: Env,
	serverId: string,
	options: { page?: number; pageSize?: number; filter?: string; before?: Date } = {}
) {
	const pageSize = [10, 20, 50, 100].includes(options.pageSize ?? 50)
		? (options.pageSize ?? 50)
		: 50;
	const filter: ActionFilter =
		options.filter === 'manual' || options.filter === 'automatic' ? options.filter : 'all';
	const now = new Date();
	const before =
		options.before && Number.isFinite(options.before.getTime()) && options.before <= now
			? options.before
			: now;
	const scope = sql`created_at<=${before} AND ${filter === 'manual' ? sql`source='REVIEW'` : filter === 'automatic' ? sql`source<>'REVIEW'` : sql`TRUE`}`;
	const [count] = await env.db.execute<{ total: number }>(
		sql`WITH history AS (${historyIndex(serverId)}) SELECT count(*)::int total FROM history WHERE ${scope}`
	);
	const total = count.total,
		pages = Math.max(1, Math.ceil(total / pageSize));
	const page = Number.isSafeInteger(options.page) ? Math.min(pages, Math.max(1, options.page!)) : 1;
	const rows = await env.db.execute<{
		id: string;
		steam_id: string;
		case_id: string | null;
		source: string;
		action: string;
		state: string;
		reason: string;
		created_at: Date;
		effective_at: Date | null;
		expires_at: Date | null;
		reverted_at: Date | null;
		score: number | null;
		threshold: number | null;
		name: string | null;
	}>(sql`WITH history AS (${historyIndex(serverId)}), selected AS (
	 SELECT * FROM history WHERE ${scope} ORDER BY created_at DESC,id DESC LIMIT ${pageSize} OFFSET ${(page - 1) * pageSize}
	 ) SELECT h.id,h.source,h.created_at,
	 CASE h.kind WHEN 'action' THEN a.steam_id WHEN 'long' THEN r.steam_id ELSE s.value->>'steamId' END AS steam_id,a.case_id,
	 CASE h.kind WHEN 'action' THEN a.action WHEN 'long' THEN r.action ELSE CASE WHEN s.value->>'attemptedAt' IS NULL THEN 'WARNING' ELSE 'KICK' END END AS action,
	 CASE h.kind WHEN 'action' THEN a.delivery_state WHEN 'long' THEN coalesce(r.action_state,'pending') ELSE coalesce(s.value->>'state','warning') END AS state,
	 CASE WHEN h.kind='short' THEN coalesce(s.value->>'reason','') ELSE coalesce((SELECT o.outcome FROM outbox o WHERE o.server_id=${serverId}
	 AND ((h.kind='action' AND o.trigger_kind='integrity' AND o.detail->>'actionId'=h.raw_id) OR (h.kind='long' AND o.trigger_kind='model_integrity' AND o.detail->>'runId'=h.raw_id))
	 ORDER BY o.created_at DESC,o.id DESC LIMIT 1),r.action_reason,'') END AS reason,
	 CASE h.kind WHEN 'action' THEN a.effective_at WHEN 'long' THEN CASE WHEN r.action_state='delivered' THEN r.punished_at ELSE NULL END ELSE CASE WHEN s.value->>'state'='delivered' THEN s.updated_at ELSE NULL END END AS effective_at,
	 coalesce(a.expires_at,r.expires_at) AS expires_at,a.reverted_at,
	 CASE WHEN h.kind='long' THEN r.score WHEN h.kind='short' THEN coalesce((s.value->>'weightedScore')::float8,(s.value->>'score')::float8) ELSE NULL END AS score,
	 CASE WHEN h.kind='long' THEN r.threshold WHEN h.kind='short' THEN CASE WHEN s.value->>'attemptedAt' IS NULL THEN ${SHORT_WARNING} ELSE ${SHORT_KICK} END ELSE NULL END AS threshold,s.value->>'name' AS name
	 FROM selected h LEFT JOIN integrity_actions a ON h.kind='action' AND a.id=h.raw_id
	 LEFT JOIN integrity_model_runs r ON h.kind='long' AND r.id=h.raw_id LEFT JOIN site_settings s ON h.kind='short' AND s.key=h.raw_id ORDER BY h.created_at DESC,h.id DESC`);
	return {
		total,
		pages,
		page,
		pageSize,
		filter,
		before: before.toISOString(),
		rows: rows.map((r) => ({
			id: r.id,
			steamId: r.steam_id,
			caseId: r.case_id,
			source: r.source,
			action: r.action,
			deliveryState: r.state,
			deliveryReason: r.reason,
			name: r.name,
			score: r.score,
			threshold: r.threshold,
			percentileLabel:
				r.source === 'LONG_MODEL'
					? r.action === 'QUARANTINE_24H'
						? 'P99'
						: r.threshold === MODEL_CALIBRATION.p98
							? 'P98'
							: r.threshold === MODEL_CALIBRATION.p97
								? 'P97'
								: r.threshold === MODEL_CALIBRATION.p95
									? 'P95'
									: '历史阈值'
					: r.source === 'SHORT_MODEL'
						? r.action === 'WARNING'
							? 'P99.6'
							: 'P99.9'
						: '',
			createdAt: new Date(r.created_at).toISOString(),
			effectiveAt: r.effective_at ? new Date(r.effective_at).toISOString() : null,
			expiresAt: r.expires_at ? new Date(r.expires_at).toISOString() : null,
			revertedAt: r.reverted_at ? new Date(r.reverted_at).toISOString() : null
		}))
	};
}
export async function integrityActionHistory(env: Env, serverId: string) {
	return (await integrityActionPage(env, serverId)).rows;
}
