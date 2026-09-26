CREATE TABLE weapon_restriction_rules (
server_id text PRIMARY KEY REFERENCES servers(id) ON DELETE CASCADE,
enabled boolean NOT NULL DEFAULT false, causes jsonb NOT NULL DEFAULT '[]', groups jsonb NOT NULL DEFAULT '[]',
updated_at timestamptz NOT NULL DEFAULT now());
--> statement-breakpoint
CREATE TABLE weapon_restriction_events (
id text PRIMARY KEY, server_id text NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
match_id bigint NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
rule_version text NOT NULL, steam_id text NOT NULL, player_name text NOT NULL, cause text NOT NULL, event_id text NOT NULL,
action text NOT NULL, state text NOT NULL, reason text NOT NULL, clock real NOT NULL,
created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL);
--> statement-breakpoint
CREATE INDEX weapon_restriction_player_idx ON weapon_restriction_events(server_id,match_id,steam_id,created_at DESC);
