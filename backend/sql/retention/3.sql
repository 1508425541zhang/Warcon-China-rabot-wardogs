CREATE TEMP TABLE history_retention_targets ON COMMIT DROP AS
		 WITH history AS (SELECT a.id,a.id AS raw_id,'action'::text AS kind,a.source,a.created_at FROM integrity_actions a WHERE a.server_id=$1
	 UNION ALL SELECT 'long:'||r.id,r.id,'long','LONG_MODEL',r.punished_at FROM integrity_model_runs r WHERE r.server_id=$1 AND r.action IS NOT NULL AND r.punished_at IS NOT NULL
	 UNION ALL SELECT 'short:'||s.key,s.key,'short','SHORT_MODEL',coalesce((s.value->>'attemptedAt')::timestamptz,(s.value->>'createdAt')::timestamptz,s.updated_at) FROM site_settings s WHERE s.key LIKE 'shortRisk:%' AND s.value->>'serverId'=$1), ranked AS (
		 SELECT *,row_number() OVER(ORDER BY created_at DESC,id DESC) AS ordinal FROM history
		 ) SELECT h.* FROM ranked h
		 WHERE (ordinal>$2 OR created_at<now()-($3*interval '1 day'))
		 AND created_at<now()-interval '7 days'
		 AND (
		 (kind='action' AND EXISTS(SELECT 1 FROM integrity_actions a WHERE a.id=h.raw_id
		  AND coalesce(a.delivery_state,CASE WHEN a.effective_at IS NOT NULL THEN 'delivered' ELSE 'unknown' END) IN ('delivered','failed','skipped')
		  AND (a.expires_at IS NULL OR a.expires_at<=now())
		  AND NOT EXISTS(SELECT 1 FROM list_entries e WHERE e.id=a.list_entry_id AND e.removed_at IS NULL AND (e.expires_at IS NULL OR e.expires_at>now()))))
		 OR (kind='long' AND EXISTS(SELECT 1 FROM integrity_model_runs r WHERE r.id=h.raw_id
		  AND r.action_state IN ('delivered','failed','skipped') AND (r.expires_at IS NULL OR r.expires_at<=now())
		  AND NOT EXISTS(SELECT 1 FROM list_entries e WHERE e.id=r.list_entry_id AND e.removed_at IS NULL AND (e.expires_at IS NULL OR e.expires_at>now()))))
		 OR (kind='short' AND EXISTS(SELECT 1 FROM site_settings s WHERE s.key=h.raw_id
		  AND coalesce(s.value->>'state','warning') IN ('warning','delivered','failed','skipped')
		  AND NOT EXISTS(SELECT 1 FROM matches m WHERE m.server_id=$1 AND m.ended_at IS NULL AND m.id::text=s.value->'scope'->>2)))
		 )
		 AND NOT EXISTS(SELECT 1 FROM outbox o WHERE o.server_id=$1
		 AND ((kind='action' AND o.trigger_kind='integrity' AND o.detail->>'actionId'=h.raw_id)
		 OR (kind='long' AND o.trigger_kind='model_integrity' AND o.detail->>'runId'=h.raw_id))
		 AND (o.state NOT IN ('delivered','failed','skipped') OR EXISTS(SELECT 1 FROM qq_integrity_notifications n WHERE n.outbox_id=o.id AND n.state NOT IN ('delivered','skipped'))))
		 ORDER BY created_at,id LIMIT 500
