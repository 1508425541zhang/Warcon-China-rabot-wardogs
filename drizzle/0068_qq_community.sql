CREATE TABLE qq_links (
 server_id text NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
 member_id text NOT NULL, user_id text NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
 steam_id text NOT NULL, PRIMARY KEY(server_id, member_id), UNIQUE(server_id, steam_id)
);
--> statement-breakpoint
CREATE TABLE qq_link_codes (
 code_hash text PRIMARY KEY, server_id text NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
 member_id text NOT NULL, expires_at timestamptz NOT NULL
);
--> statement-breakpoint
CREATE TABLE qq_wallets (
 server_id text NOT NULL REFERENCES servers(id) ON DELETE CASCADE, steam_id text NOT NULL,
 balance bigint NOT NULL DEFAULT 0 CHECK(balance >= 0), warm_ms bigint NOT NULL DEFAULT 0,
 PRIMARY KEY(server_id, steam_id)
);
--> statement-breakpoint
CREATE TABLE qq_ledger (
 id text PRIMARY KEY, server_id text NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
 steam_id text NOT NULL, delta bigint NOT NULL, reason text NOT NULL, created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX qq_ledger_player_idx ON qq_ledger(server_id, steam_id, created_at);
--> statement-breakpoint
CREATE TABLE qq_warm_ticks (server_id text PRIMARY KEY REFERENCES servers(id) ON DELETE CASCADE, observed_at timestamptz NOT NULL);
--> statement-breakpoint
CREATE TABLE qq_inbox (
 id text PRIMARY KEY, server_id text NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
 group_id text NOT NULL, member_id text NOT NULL, content text NOT NULL,
 state text NOT NULL DEFAULT 'pending', reply text, reply_state text NOT NULL DEFAULT 'pending',
 created_at timestamptz NOT NULL DEFAULT now(), started_at timestamptz
);
CREATE INDEX qq_inbox_pending_idx ON qq_inbox(state, created_at);
--> statement-breakpoint
CREATE TABLE qq_orders (
 id text PRIMARY KEY, server_id text NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
 steam_id text NOT NULL, kind text NOT NULL, params jsonb NOT NULL, cost bigint NOT NULL,
 state text NOT NULL DEFAULT 'pending', outcome text, created_at timestamptz NOT NULL DEFAULT now(), started_at timestamptz
);
CREATE INDEX qq_orders_pending_idx ON qq_orders(state, created_at);
--> statement-breakpoint
CREATE TABLE qq_deliveries (
 order_id text NOT NULL REFERENCES qq_orders(id) ON DELETE CASCADE, steam_id text NOT NULL,
 state text NOT NULL DEFAULT 'pending', outcome text, PRIMARY KEY(order_id, steam_id)
);
--> statement-breakpoint
CREATE TABLE qq_votes (
 id text PRIMARY KEY, server_id text NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
 maps jsonb NOT NULL, ends_at timestamptz NOT NULL, state text NOT NULL DEFAULT 'open', winner text
);
CREATE UNIQUE INDEX qq_votes_open_idx ON qq_votes(server_id) WHERE state = 'open';
--> statement-breakpoint
CREATE TABLE qq_ballots (
 vote_id text NOT NULL REFERENCES qq_votes(id) ON DELETE CASCADE, steam_id text NOT NULL,
 map text NOT NULL, PRIMARY KEY(vote_id, steam_id)
);
