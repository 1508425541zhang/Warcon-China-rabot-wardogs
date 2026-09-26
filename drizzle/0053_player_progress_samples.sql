CREATE TABLE "player_progress_samples" (
 "server_id" text NOT NULL,
 "match_id" bigint NOT NULL REFERENCES "matches"("id") ON DELETE CASCADE,
 "bucket" bigint NOT NULL,
 "observed_at" timestamptz NOT NULL,
 "players" jsonb NOT NULL,
 PRIMARY KEY ("server_id", "match_id", "bucket")
);
--> statement-breakpoint
CREATE INDEX "player_progress_match_idx" ON "player_progress_samples" ("match_id", "observed_at");
