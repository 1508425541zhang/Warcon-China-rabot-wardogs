CREATE TEMP TABLE history_retention_outbox ON COMMIT DROP AS SELECT o.id FROM outbox o
		 JOIN history_retention_targets h ON (h.kind='action' AND o.trigger_kind='integrity' AND o.detail->>'actionId'=h.raw_id)
		 OR (h.kind='long' AND o.trigger_kind='model_integrity' AND o.detail->>'runId'=h.raw_id)
		 WHERE o.server_id=$1
