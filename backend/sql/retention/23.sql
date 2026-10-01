DELETE FROM audit_log WHERE server_id=$1 AND ts<now()-interval '7 days' AND detail->>'caseId' IN (SELECT id FROM history_retention_cases)
