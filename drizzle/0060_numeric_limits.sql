CREATE TABLE numeric_limit_rules (
 server_id text PRIMARY KEY REFERENCES servers(id) ON DELETE CASCADE,
 config jsonb NOT NULL,
 updated_at timestamptz NOT NULL DEFAULT now()
);
--> statement-breakpoint
CREATE TABLE numeric_limit_events (
 id text PRIMARY KEY,
 server_id text NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
 match_id bigint NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
 steam_id text NOT NULL,
 rule_version text NOT NULL,
 action text NOT NULL,
 state text NOT NULL,
 evidence jsonb NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now()
);
--> statement-breakpoint
CREATE INDEX numeric_limit_player_idx ON numeric_limit_events(server_id, match_id, steam_id, created_at);
