CREATE TABLE skill_balance_rules (
 server_id text PRIMARY KEY REFERENCES servers(id) ON DELETE CASCADE,
 enabled boolean NOT NULL DEFAULT false,
 grace_seconds integer NOT NULL DEFAULT 300 CHECK(grace_seconds BETWEEN 180 AND 1800),
 lead_points integer NOT NULL DEFAULT 40 CHECK(lead_points BETWEEN 40 AND 10000),
 updated_at timestamptz NOT NULL DEFAULT now()
);
--> statement-breakpoint
CREATE TABLE skill_balance_runs (
 id text PRIMARY KEY,
 server_id text NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
 match_id bigint NOT NULL,
 state text NOT NULL,
 reason text NOT NULL,
 plan jsonb NOT NULL,
 moves jsonb NOT NULL DEFAULT '[]',
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now()
);
--> statement-breakpoint
CREATE UNIQUE INDEX skill_balance_round_unique ON skill_balance_runs(server_id,match_id);
