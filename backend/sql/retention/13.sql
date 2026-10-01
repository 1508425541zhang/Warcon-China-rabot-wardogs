CREATE TEMP TABLE history_retention_cases ON COMMIT DROP AS SELECT DISTINCT a.case_id AS id
		 FROM integrity_actions a JOIN history_retention_targets h ON h.kind='action' AND h.raw_id=a.id
