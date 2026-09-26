-- Only factions read from a fresh player list may be verified by a later look.
-- Older session-derived values remain visible but cannot become Integrity evidence.
ALTER TABLE "kills" ADD COLUMN "faction_observed_at" timestamptz;--> statement-breakpoint
-- Existing generations may include session-derived faction labels; rebuild with roster evidence.
UPDATE "integrity_model_state" SET "baseline_status" = 'STALE', "updated_at" = now();
