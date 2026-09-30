CREATE INDEX integrity_actions_server_time_idx ON integrity_actions(server_id, created_at DESC, id DESC);
--> statement-breakpoint
CREATE INDEX integrity_model_runs_history_idx ON integrity_model_runs(server_id, punished_at DESC, id DESC) WHERE action IS NOT NULL AND punished_at IS NOT NULL;
--> statement-breakpoint
CREATE INDEX integrity_model_runs_retention_idx ON integrity_model_runs(server_id, created_at DESC, id DESC);
--> statement-breakpoint
CREATE INDEX short_risk_history_idx ON site_settings((value->>'serverId'), updated_at DESC) WHERE key LIKE 'shortRisk:%';
--> statement-breakpoint
CREATE INDEX integrity_outbox_action_idx ON outbox(server_id, (detail->>'actionId')) WHERE trigger_kind='integrity';
--> statement-breakpoint
CREATE INDEX integrity_outbox_model_idx ON outbox(server_id, (detail->>'runId')) WHERE trigger_kind='model_integrity';
