DELETE FROM integrity_model_runs WHERE id IN (SELECT raw_id FROM history_retention_targets WHERE kind='long' UNION SELECT id FROM history_retention_runs) RETURNING id
