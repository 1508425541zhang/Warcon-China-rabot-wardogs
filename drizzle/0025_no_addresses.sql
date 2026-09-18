-- People's addresses are no longer kept anywhere: the audit trail loses the column, sign-in
-- sessions lose theirs, and the login lockout starts again on keyed hashes.
ALTER TABLE "audit_log" DROP COLUMN "ip";--> statement-breakpoint
UPDATE "session" SET "ip_address" = NULL;--> statement-breakpoint
DELETE FROM "login_attempts" WHERE "key" LIKE 'ip:%' OR "key" LIKE 'signup:%';
