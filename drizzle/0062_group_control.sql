CREATE TABLE group_control_rules (
 server_id text PRIMARY KEY REFERENCES servers(id) ON DELETE CASCADE,
 config jsonb NOT NULL,
 updated_at timestamptz NOT NULL DEFAULT now()
);
--> statement-breakpoint
CREATE TABLE group_control_scans (
 server_id text PRIMARY KEY REFERENCES servers(id) ON DELETE CASCADE,
 config jsonb NOT NULL,
 groups jsonb NOT NULL,
 scanned_at timestamptz NOT NULL,
 ai_status text NOT NULL DEFAULT 'not_requested',
 ai_result jsonb,
 ai_fingerprint text
);
