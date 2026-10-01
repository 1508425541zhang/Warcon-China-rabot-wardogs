DELETE FROM integrity_labels WHERE case_id IN (SELECT id FROM history_retention_cases)
