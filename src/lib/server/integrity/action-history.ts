import { sql } from 'drizzle-orm';
import type { Env } from '../env';
import { MODEL_CALIBRATION } from './model-http';
import { SHORT_KICK, SHORT_WARNING } from '$lib/short-risk-policy';
/** Read-only projection: retain original action IDs, sources and delivery outcomes. */
export async function integrityActionHistory(env: Env, serverId: string) {
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
	}>(sql`
 WITH history AS (
 SELECT a.id,a.steam_id,a.case_id,a.source,a.action,a.delivery_state AS state,
 coalesce((SELECT o.outcome FROM outbox o WHERE o.server_id=a.server_id AND o.trigger_kind='integrity' AND o.detail->>'actionId'=a.id ORDER BY o.created_at DESC,o.id DESC LIMIT 1),'') AS reason,
 a.created_at,a.effective_at,a.expires_at,a.reverted_at,NULL::float8 AS score,NULL::float8 AS threshold,NULL::text AS name
 FROM integrity_actions a WHERE a.server_id=${serverId}
 UNION ALL
 SELECT 'long:'||r.id,r.steam_id,NULL,'LONG_MODEL',r.action,coalesce(r.action_state,'pending'),
 coalesce((SELECT o.outcome FROM outbox o WHERE o.server_id=r.server_id AND o.trigger_kind='model_integrity' AND o.detail->>'runId'=r.id ORDER BY o.created_at DESC,o.id DESC LIMIT 1),r.action_reason,''),
 r.punished_at,CASE WHEN r.action_state='delivered' THEN r.punished_at ELSE NULL END,r.expires_at,NULL,r.score,r.threshold,NULL
 FROM integrity_model_runs r WHERE r.server_id=${serverId} AND r.action IS NOT NULL AND r.punished_at IS NOT NULL
 UNION ALL
 SELECT 'short:'||s.key,s.value->>'steamId',NULL,'SHORT_MODEL',
 CASE WHEN s.value->>'attemptedAt' IS NULL THEN 'WARNING' ELSE 'KICK' END,
 coalesce(s.value->>'state','warning'),coalesce(s.value->>'reason',''),
 coalesce((s.value->>'attemptedAt')::timestamptz,s.updated_at),
 CASE WHEN s.value->>'state'='delivered' THEN s.updated_at ELSE NULL END,NULL,NULL,
 coalesce((s.value->>'weightedScore')::float8,(s.value->>'score')::float8),
 CASE WHEN s.value->>'attemptedAt' IS NULL THEN ${SHORT_WARNING} ELSE ${SHORT_KICK} END,s.value->>'name'
 FROM site_settings s WHERE s.key LIKE 'shortRisk:%' AND s.value->>'serverId'=${serverId}
 ) SELECT * FROM history ORDER BY created_at DESC,id DESC LIMIT 50
 `);
	return rows.map((r) => ({
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
	}));
}
