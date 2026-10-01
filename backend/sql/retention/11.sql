DELETE FROM history_retention_targets h WHERE NOT ((
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
		 AND (o.state NOT IN ('delivered','failed','skipped') OR EXISTS(SELECT 1 FROM qq_integrity_notifications n WHERE n.outbox_id=o.id AND n.state NOT IN ('delivered','skipped')))))
