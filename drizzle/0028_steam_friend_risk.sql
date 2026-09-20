ALTER TABLE "steam_profiles" ADD COLUMN "friends_state" text DEFAULT 'unknown' NOT NULL;--> statement-breakpoint
ALTER TABLE "steam_profiles" ADD COLUMN "friends_total" integer DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "steam_profiles" ADD COLUMN "friends_checked" integer DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "steam_profiles" ADD COLUMN "banned_friends" integer DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "steam_profiles" ADD COLUMN "friends_checked_at" timestamp with time zone;
ALTER TABLE "player_marks" ADD COLUMN "risk" jsonb;--> statement-breakpoint
ALTER TABLE "player_marks" ADD COLUMN "risk_scored_at" timestamp with time zone;
