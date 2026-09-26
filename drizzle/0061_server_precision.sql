ALTER TABLE integrity_baselines ADD COLUMN server_id text;
--> statement-breakpoint
CREATE INDEX integrity_baselines_server_idx ON integrity_baselines(org_id,server_id,metric);
