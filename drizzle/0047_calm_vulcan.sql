CREATE TABLE "integrity_action_eligibility" (
	"case_id" text PRIMARY KEY NOT NULL,
	"last_attempt_at" timestamp with time zone NOT NULL,
	"attempts" integer DEFAULT 1 NOT NULL
);
--> statement-breakpoint
CREATE TABLE "integrity_labels" (
	"id" bigserial PRIMARY KEY NOT NULL,
	"case_id" text NOT NULL,
	"org_id" text NOT NULL,
	"label" text NOT NULL,
	"reason" text NOT NULL,
	"reviewer_id" text NOT NULL,
	"model_version" text NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "integrity_model_state" (
	"org_id" text PRIMARY KEY NOT NULL,
	"weapon_map_version" integer DEFAULT 1 NOT NULL,
	"active_baseline_generation" text,
	"baseline_status" text DEFAULT 'STALE' NOT NULL,
	"last_refresh_at" timestamp with time zone,
	"last_failure_at" timestamp with time zone,
	"last_duration_ms" integer,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "integrity_player_metric_history" (
	"id" bigserial PRIMARY KEY NOT NULL,
	"org_id" text NOT NULL,
	"steam_id" text NOT NULL,
	"server_id" text NOT NULL,
	"round_id" text NOT NULL,
	"event_id" text NOT NULL,
	"observed_at" timestamp with time zone NOT NULL,
	"kpm_180" real NOT NULL,
	"headshot_rate" real,
	"max_kills_15s" integer NOT NULL,
	"feature_version" text NOT NULL,
	"model_version" text NOT NULL
);
--> statement-breakpoint
CREATE TABLE "integrity_profile_refresh_jobs" (
	"steam_id" text PRIMARY KEY NOT NULL,
	"state" text DEFAULT 'pending' NOT NULL,
	"attempts" integer DEFAULT 0 NOT NULL,
	"next_at" timestamp with time zone DEFAULT now() NOT NULL,
	"lease_until" timestamp with time zone,
	"last_error" text,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
DROP INDEX "feed_processing_pending_idx";--> statement-breakpoint
ALTER TABLE "feed_processing_jobs" ADD COLUMN "consumer" text DEFAULT 'legacy' NOT NULL;--> statement-breakpoint
ALTER TABLE "integrity_baselines" ADD COLUMN "unique_players" integer DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "integrity_baselines" ADD COLUMN "unique_player_days" integer DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "integrity_baselines" ADD COLUMN "effective_sample_size" real DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "integrity_baselines" ADD COLUMN "model_version" text DEFAULT 'legacy-fixed-v0' NOT NULL;--> statement-breakpoint
ALTER TABLE "integrity_baselines" ADD COLUMN "feature_version" text DEFAULT 'fixed-slot-v0' NOT NULL;--> statement-breakpoint
ALTER TABLE "integrity_baselines" ADD COLUMN "weapon_map_version" integer DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "integrity_baselines" ADD COLUMN "generation" text DEFAULT 'legacy' NOT NULL;--> statement-breakpoint
ALTER TABLE "kills" ADD COLUMN "distance_invalid" boolean DEFAULT false NOT NULL;--> statement-breakpoint
ALTER TABLE "kills" ADD COLUMN "raw_distance_cm" real;--> statement-breakpoint
ALTER TABLE "integrity_action_eligibility" ADD CONSTRAINT "integrity_action_eligibility_case_id_integrity_cases_id_fk" FOREIGN KEY ("case_id") REFERENCES "public"."integrity_cases"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "integrity_labels" ADD CONSTRAINT "integrity_labels_org_id_organizations_id_fk" FOREIGN KEY ("org_id") REFERENCES "public"."organizations"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "integrity_model_state" ADD CONSTRAINT "integrity_model_state_org_id_organizations_id_fk" FOREIGN KEY ("org_id") REFERENCES "public"."organizations"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "integrity_player_metric_history" ADD CONSTRAINT "integrity_player_metric_history_org_id_organizations_id_fk" FOREIGN KEY ("org_id") REFERENCES "public"."organizations"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
CREATE INDEX "integrity_labels_case_idx" ON "integrity_labels" USING btree ("case_id","created_at" DESC NULLS LAST);--> statement-breakpoint
CREATE UNIQUE INDEX "integrity_player_history_event_idx" ON "integrity_player_metric_history" USING btree ("org_id","server_id","event_id");--> statement-breakpoint
CREATE INDEX "integrity_player_history_player_idx" ON "integrity_player_metric_history" USING btree ("org_id","steam_id","observed_at" DESC NULLS LAST);--> statement-breakpoint
CREATE INDEX "integrity_profile_refresh_pending_idx" ON "integrity_profile_refresh_jobs" USING btree ("state","next_at");--> statement-breakpoint
CREATE INDEX "feed_processing_pending_idx" ON "feed_processing_jobs" USING btree ("consumer","state","created_at");