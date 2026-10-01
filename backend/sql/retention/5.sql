SELECT c.id FROM integrity_cases c WHERE c.id IN (SELECT a.case_id FROM integrity_actions a JOIN history_retention_targets h ON h.kind='action' AND h.raw_id=a.id) ORDER BY c.id FOR UPDATE
