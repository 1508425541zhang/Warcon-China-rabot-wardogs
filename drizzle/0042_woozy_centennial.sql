CREATE TABLE "integrity_actions" (
	"id" text PRIMARY KEY NOT NULL,
	"case_id" text NOT NULL,
	"org_id" text NOT NULL,
	"server_id" text NOT NULL,
	"steam_id" text NOT NULL,
	"action" text NOT NULL,
	"source" text NOT NULL,
	"list_entry_id" text,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"expires_at" timestamp with time zone,
	"reverted_at" timestamp with time zone,
	"reverted_by" text
);
--> statement-breakpoint
CREATE TABLE "integrity_case_events" (
	"case_id" text NOT NULL,
	"instance_id" text NOT NULL,
	"event_id" text NOT NULL,
	"event" jsonb NOT NULL,
	CONSTRAINT "integrity_case_events_case_id_instance_id_event_id_pk" PRIMARY KEY("case_id","instance_id","event_id")
);
--> statement-breakpoint
ALTER TABLE "integrity_rules" ADD COLUMN "auto_kick_enabled" boolean DEFAULT false NOT NULL;--> statement-breakpoint
ALTER TABLE "integrity_rules" ADD COLUMN "auto_quarantine_24h_enabled" boolean DEFAULT false NOT NULL;--> statement-breakpoint
ALTER TABLE "integrity_rules" ADD COLUMN "auto_quarantine_7d_enabled" boolean DEFAULT false NOT NULL;--> statement-breakpoint
ALTER TABLE "integrity_rules" ADD COLUMN "auto_action_max_per_hour" integer DEFAULT 10 NOT NULL;--> statement-breakpoint
ALTER TABLE "integrity_rules" ADD COLUMN "auto_action_max_percent_online" integer DEFAULT 10 NOT NULL;--> statement-breakpoint
ALTER TABLE "integrity_rules" ADD COLUMN "auto_suspended_at" timestamp with time zone;--> statement-breakpoint
ALTER TABLE "integrity_windows" ADD COLUMN "headshots" integer DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "integrity_windows" ADD COLUMN "penetrations" integer DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "integrity_windows" ADD COLUMN "burst_points" integer DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "integrity_windows" ADD COLUMN "behavior_reasons" jsonb DEFAULT '[]'::jsonb NOT NULL;--> statement-breakpoint
ALTER TABLE "integrity_actions" ADD CONSTRAINT "integrity_actions_case_id_integrity_cases_id_fk" FOREIGN KEY ("case_id") REFERENCES "public"."integrity_cases"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "integrity_case_events" ADD CONSTRAINT "integrity_case_events_case_id_integrity_cases_id_fk" FOREIGN KEY ("case_id") REFERENCES "public"."integrity_cases"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
CREATE INDEX "integrity_actions_player_idx" ON "integrity_actions" USING btree ("org_id","steam_id","created_at" DESC NULLS LAST);--> statement-breakpoint
-- Repair recoverable legacy provenance, and retain rows with no usable source in an archive.
UPDATE "integrity_scores" SET "source" = 'report' WHERE "source" = 'window' AND "window_id" IS NULL AND "report_id" IS NOT NULL;--> statement-breakpoint
UPDATE "integrity_scores" SET "source" = 'window' WHERE "source" = 'report' AND "report_id" IS NULL AND "window_id" IS NOT NULL;--> statement-breakpoint
UPDATE "integrity_scores" SET "report_id" = NULL WHERE "source" = 'window' AND "window_id" IS NOT NULL AND "report_id" IS NOT NULL;--> statement-breakpoint
UPDATE "integrity_scores" SET "window_id" = NULL WHERE "source" = 'report' AND "report_id" IS NOT NULL AND "window_id" IS NOT NULL;--> statement-breakpoint
CREATE TABLE "integrity_scores_orphaned_0042" AS SELECT * FROM "integrity_scores" WHERE NOT (("source" = 'window' AND "window_id" IS NOT NULL AND "report_id" IS NULL) OR ("source" = 'report' AND "report_id" IS NOT NULL AND "window_id" IS NULL));--> statement-breakpoint
DELETE FROM "integrity_scores" WHERE NOT (("source" = 'window' AND "window_id" IS NOT NULL AND "report_id" IS NULL) OR ("source" = 'report' AND "report_id" IS NOT NULL AND "window_id" IS NULL));--> statement-breakpoint
ALTER TABLE "integrity_scores" ADD CONSTRAINT "integrity_scores_source_check" CHECK (("integrity_scores"."source" = 'window' AND "integrity_scores"."window_id" IS NOT NULL AND "integrity_scores"."report_id" IS NULL) OR ("integrity_scores"."source" = 'report' AND "integrity_scores"."report_id" IS NOT NULL AND "integrity_scores"."window_id" IS NULL));
