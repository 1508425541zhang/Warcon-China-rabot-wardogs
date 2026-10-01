SELECT s.key FROM site_settings s JOIN history_retention_targets h ON h.kind='short' AND h.raw_id=s.key ORDER BY s.key FOR UPDATE OF s
