DELETE FROM site_settings WHERE key IN (SELECT raw_id FROM history_retention_targets WHERE kind='short') RETURNING key
