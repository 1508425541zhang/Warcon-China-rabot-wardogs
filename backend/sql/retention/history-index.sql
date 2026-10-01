SELECT a.id,a.id AS raw_id,'action'::text AS kind,a.source,a.created_at FROM integrity_actions a WHERE a.server_id=$1
	 UNION ALL SELECT 'long:'||r.id,r.id,'long','LONG_MODEL',r.punished_at FROM integrity_model_runs r WHERE r.server_id=$1 AND r.action IS NOT NULL AND r.punished_at IS NOT NULL
	 UNION ALL SELECT 'short:'||s.key,s.key,'short','SHORT_MODEL',coalesce((s.value->>'attemptedAt')::timestamptz,(s.value->>'createdAt')::timestamptz,s.updated_at) FROM site_settings s WHERE s.key LIKE 'shortRisk:%' AND s.value->>'serverId'=$1
