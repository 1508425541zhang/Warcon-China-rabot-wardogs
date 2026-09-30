CREATE TABLE integrity_model_runs (
 id text PRIMARY KEY,
 org_id text NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
 server_id text NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
 steam_id text NOT NULL,
 match_id bigint NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
 slot bigint NOT NULL,
 config_revision text NOT NULL,
 state text NOT NULL DEFAULT 'pending',
 score double precision,
 threshold double precision NOT NULL,
 result jsonb,
 created_at timestamptz NOT NULL DEFAULT now(),
 finished_at timestamptz,
 action text,
 action_state text,
 action_reason text,
 punished_at timestamptz,
 expires_at timestamptz,
 list_entry_id text REFERENCES list_entries(id),
 UNIQUE(server_id, steam_id, match_id, slot, config_revision)
);
CREATE INDEX integrity_model_runs_org_time_idx ON integrity_model_runs(org_id, created_at DESC);
CREATE INDEX integrity_model_runs_action_idx ON integrity_model_runs(server_id, steam_id, punished_at DESC);
