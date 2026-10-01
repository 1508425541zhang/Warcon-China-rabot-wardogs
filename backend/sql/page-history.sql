SELECT to_jsonb(rows) FROM (WITH history AS (SELECT a.id,a.id AS raw_id,'action'::text AS kind,a.source,a.created_at FROM integrity_actions a WHERE a.server_id=$1
	 UNION ALL SELECT 'long:'||r.id,r.id,'long','LONG_MODEL',r.punished_at FROM integrity_model_runs r WHERE r.server_id=$1 AND r.action IS NOT NULL AND r.punished_at IS NOT NULL
	 UNION ALL SELECT 'short:'||s.key,s.key,'short','SHORT_MODEL',coalesce((s.value->>'attemptedAt')::timestamptz,(s.value->>'createdAt')::timestamptz,s.updated_at) FROM site_settings s WHERE s.key LIKE 'shortRisk:%' AND s.value->>'serverId'=$1), selected AS (
	 SELECT * FROM history WHERE created_at<=$2 AND ($3='all' OR ($3='manual' AND source='REVIEW') OR ($3='automatic' AND source<>'REVIEW')) ORDER BY created_at DESC,id DESC LIMIT $4 OFFSET $5
	 ) SELECT h.id,h.source,h.created_at,
	 CASE h.kind WHEN 'action' THEN a.steam_id WHEN 'long' THEN r.steam_id ELSE s.value->>'steamId' END AS steam_id,a.case_id,
	 CASE h.kind WHEN 'action' THEN a.action WHEN 'long' THEN r.action ELSE CASE WHEN s.value->>'attemptedAt' IS NULL THEN 'WARNING' ELSE 'KICK' END END AS action,
	 CASE h.kind WHEN 'action' THEN a.delivery_state WHEN 'long' THEN coalesce(r.action_state,'pending') ELSE coalesce(s.value->>'state','warning') END AS state,
	 CASE WHEN h.kind='short' THEN coalesce(s.value->>'reason','') ELSE coalesce((SELECT o.outcome FROM outbox o WHERE o.server_id=$1
	 AND ((h.kind='action' AND o.trigger_kind='integrity' AND o.detail->>'actionId'=h.raw_id) OR (h.kind='long' AND o.trigger_kind='model_integrity' AND o.detail->>'runId'=h.raw_id))
	 ORDER BY o.created_at DESC,o.id DESC LIMIT 1),r.action_reason,'') END AS reason,
	 CASE h.kind WHEN 'action' THEN a.effective_at WHEN 'long' THEN CASE WHEN r.action_state='delivered' THEN r.punished_at ELSE NULL END ELSE CASE WHEN s.value->>'state'='delivered' THEN s.updated_at ELSE NULL END END AS effective_at,
	 coalesce(a.expires_at,r.expires_at) AS expires_at,a.reverted_at,
	 CASE WHEN h.kind='long' THEN r.score WHEN h.kind='short' THEN coalesce((s.value->>'weightedScore')::float8,(s.value->>'score')::float8) ELSE NULL END AS score,
	 CASE WHEN h.kind='long' THEN r.threshold WHEN h.kind='short' THEN CASE WHEN s.value->>'attemptedAt' IS NULL THEN $6::float8 ELSE $7::float8 END ELSE NULL END AS threshold,s.value->>'name' AS name
	 FROM selected h LEFT JOIN integrity_actions a ON h.kind='action' AND a.id=h.raw_id
	 LEFT JOIN integrity_model_runs r ON h.kind='long' AND r.id=h.raw_id LEFT JOIN site_settings s ON h.kind='short' AND s.key=h.raw_id ORDER BY h.created_at DESC,h.id DESC) rows
