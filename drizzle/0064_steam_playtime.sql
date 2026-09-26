CREATE TABLE steam_game_playtime (
 steam_id text PRIMARY KEY,
 minutes integer CHECK (minutes >= 0),
 state text NOT NULL DEFAULT 'pending',
 checked_at timestamptz,
 next_at timestamptz NOT NULL DEFAULT now(),
 lease_until timestamptz
);
CREATE INDEX steam_game_playtime_next_idx ON steam_game_playtime(next_at);
