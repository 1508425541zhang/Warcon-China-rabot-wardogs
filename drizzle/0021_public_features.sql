ALTER TABLE "organizations" ADD COLUMN "allow_public_status" boolean DEFAULT false NOT NULL;--> statement-breakpoint
ALTER TABLE "organizations" ADD COLUMN "allow_public_leaderboards" boolean DEFAULT false NOT NULL;--> statement-breakpoint
ALTER TABLE "organizations" ADD COLUMN "discord_invite_url" text DEFAULT '' NOT NULL;--> statement-breakpoint
ALTER TABLE "servers" ADD COLUMN "public_status" boolean DEFAULT false NOT NULL;--> statement-breakpoint
ALTER TABLE "servers" ADD COLUMN "public_leaderboards" boolean DEFAULT false NOT NULL;--> statement-breakpoint
ALTER TABLE "webhooks" ADD COLUMN "status_interval_s" integer DEFAULT 60 NOT NULL;--> statement-breakpoint
ALTER TABLE "webhooks" ADD COLUMN "link_status" boolean DEFAULT true NOT NULL;--> statement-breakpoint
ALTER TABLE "webhooks" ADD COLUMN "link_leaderboard" boolean DEFAULT true NOT NULL;--> statement-breakpoint
ALTER TABLE "webhooks" ADD COLUMN "link_panel" boolean DEFAULT false NOT NULL;--> statement-breakpoint
-- Cards that already exist were linking their title to the panel; keep that for them. New
-- webhooks start with the panel link off (staff channels), the public links on.
UPDATE "webhooks" SET "link_panel" = true WHERE "status_enabled";
