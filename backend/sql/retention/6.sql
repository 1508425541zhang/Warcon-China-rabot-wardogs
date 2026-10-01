SELECT a.id FROM integrity_actions a JOIN history_retention_targets h ON h.kind='action' AND h.raw_id=a.id ORDER BY a.id FOR UPDATE OF a
