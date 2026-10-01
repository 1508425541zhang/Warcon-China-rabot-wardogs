SELECT r.id FROM integrity_model_runs r WHERE r.id IN (SELECT raw_id FROM history_retention_targets WHERE kind='long' UNION SELECT id FROM history_retention_runs) ORDER BY r.id FOR UPDATE
