ALTER TABLE "organizations" ALTER COLUMN "allow_public_status" SET DEFAULT true;--> statement-breakpoint
ALTER TABLE "organizations" ALTER COLUMN "allow_public_leaderboards" SET DEFAULT true;--> statement-breakpoint
-- Organisations open their own public pages; the site owner's switch is a withdrawal, not a grant.
UPDATE "organizations" SET "allow_public_status" = true, "allow_public_leaderboards" = true;
