CREATE TABLE "integrity_cases" (
	"id" text PRIMARY KEY NOT NULL,
	"org_id" text NOT NULL,
	"server_id" text NOT NULL,
	"steam_id" text NOT NULL,
	"created_at" timestamp with time zone NOT NULL,
	"status" text DEFAULT 'OPEN' NOT NULL,
	"confidence" text NOT NULL,
	"trigger" text NOT NULL,
	"rule_version" integer NOT NULL,
	"risk_score" integer NOT NULL,
	"risk_breakdown" jsonb NOT NULL,
	"snapshot" jsonb NOT NULL,
	"reviewed_by" text,
	"reviewed_at" timestamp with time zone
);
--> statement-breakpoint
CREATE INDEX "integrity_cases_queue_idx" ON "integrity_cases" USING btree ("org_id","status","created_at" DESC NULLS LAST);--> statement-breakpoint
CREATE INDEX "integrity_cases_player_idx" ON "integrity_cases" USING btree ("org_id","steam_id","created_at" DESC NULLS LAST);