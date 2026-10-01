DELETE FROM outbox WHERE id IN (SELECT id FROM history_retention_outbox)
