DELETE FROM integrity_actions WHERE id IN (SELECT raw_id FROM history_retention_targets WHERE kind='action') RETURNING id
