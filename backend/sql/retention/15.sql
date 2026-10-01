DELETE FROM audit_log a WHERE a.server_id=$1 AND a.ts<now()-interval '7 days'
		 AND (a.detail->>'actionId' IN (SELECT raw_id FROM history_retention_targets WHERE kind='action')
		 OR a.detail->>'runId' IN (SELECT raw_id FROM history_retention_targets WHERE kind='long' UNION SELECT id FROM history_retention_runs)
		 OR a.detail->>'outboxId' IN (SELECT id::text FROM history_retention_outbox)
		 OR (a.action='shortRisk.kick' AND EXISTS(SELECT 1 FROM site_settings s JOIN history_retention_targets h ON h.kind='short' AND h.raw_id=s.key WHERE a.detail->>'steamId'=s.value->>'steamId' AND a.detail->'scope'=s.value->'scope')))
