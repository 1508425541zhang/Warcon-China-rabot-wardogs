CREATE TABLE "integrity_reporter_stats" (
	"org_id" text NOT NULL,
	"steam_id" text NOT NULL,
	"reports_submitted" integer DEFAULT 0 NOT NULL,
	"reports_confirmed" integer DEFAULT 0 NOT NULL,
	"reports_dismissed" integer DEFAULT 0 NOT NULL,
	CONSTRAINT "integrity_reporter_stats_org_id_steam_id_pk" PRIMARY KEY("org_id","steam_id")
);
--> statement-breakpoint
CREATE TABLE "integrity_reports" (
	"id" bigserial PRIMARY KEY NOT NULL,
	"org_id" text NOT NULL,
	"server_id" text NOT NULL,
	"target_steam_id" text NOT NULL,
	"reporter_steam_id" text NOT NULL,
	"reason" text NOT NULL,
	"source" text NOT NULL,
	"created_at" timestamp with time zone NOT NULL,
	"evidence_from" timestamp with time zone NOT NULL,
	"evidence_until" timestamp with time zone NOT NULL,
	"case_id" text,
	"status" text DEFAULT 'OPEN' NOT NULL
);
--> statement-breakpoint
ALTER TABLE "integrity_scores" ALTER COLUMN "window_id" DROP NOT NULL;--> statement-breakpoint
ALTER TABLE "integrity_scores" ADD COLUMN "report_id" bigint;--> statement-breakpoint
ALTER TABLE "integrity_scores" ADD COLUMN "source" text DEFAULT 'window' NOT NULL;--> statement-breakpoint
CREATE INDEX "integrity_reports_target_idx" ON "integrity_reports" USING btree ("org_id","target_steam_id","created_at" DESC NULLS LAST);--> statement-breakpoint
CREATE INDEX "integrity_reports_reporter_idx" ON "integrity_reports" USING btree ("reporter_steam_id","created_at" DESC NULLS LAST);