DELETE FROM integrity_cases WHERE id IN (SELECT id FROM history_retention_cases) RETURNING id
