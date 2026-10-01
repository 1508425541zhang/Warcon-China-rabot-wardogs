UPDATE integrity_reports SET case_id=NULL WHERE case_id IN (SELECT id FROM history_retention_cases)
