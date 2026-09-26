CREATE TABLE faction_lock_rules (server_id text PRIMARY KEY REFERENCES servers(id) ON DELETE CASCADE, enabled boolean NOT NULL DEFAULT false, grace_seconds integer NOT NULL DEFAULT 120, capacities jsonb NOT NULL DEFAULT '{}', updated_at timestamptz NOT NULL DEFAULT now());
--> statement-breakpoint
CREATE TABLE faction_move_permits (id bigserial PRIMARY KEY,server_id text NOT NULL,steam_id text NOT NULL,faction text NOT NULL,created_at timestamptz NOT NULL DEFAULT now(),expires_at timestamptz NOT NULL);
--> statement-breakpoint
CREATE INDEX faction_permit_lookup_idx ON faction_move_permits(server_id,steam_id,expires_at);
--> statement-breakpoint
CREATE TABLE faction_lock_events(id bigserial PRIMARY KEY,server_id text NOT NULL REFERENCES servers(id) ON DELETE CASCADE,match_id bigint NOT NULL,steam_id text NOT NULL,from_faction text NOT NULL,to_faction text NOT NULL,state text NOT NULL,reason text NOT NULL,created_at timestamptz NOT NULL DEFAULT now(),updated_at timestamptz NOT NULL DEFAULT now());
--> statement-breakpoint
CREATE INDEX faction_lock_pending_idx ON faction_lock_events(server_id,state,created_at);
