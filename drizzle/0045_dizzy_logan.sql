CREATE TABLE "integrity_baselines" (
	"id" text PRIMARY KEY NOT NULL,
	"org_id" text NOT NULL,
	"metric" text NOT NULL,
	"level" integer NOT NULL,
	"map" text,
	"population_bucket" text,
	"weapon_category" text NOT NULL,
	"sample_count" integer NOT NULL,
	"median" real NOT NULL,
	"mad" real,
	"p90" real NOT NULL,
	"p95" real NOT NULL,
	"p99" real NOT NULL,
	"p995" real NOT NULL,
	"p999" real NOT NULL,
	"p9995" real NOT NULL,
	"histogram" jsonb NOT NULL,
	"cdf" jsonb NOT NULL,
	"window_days" integer DEFAULT 30 NOT NULL,
	"calculated_at" timestamp with time zone NOT NULL
);
--> statement-breakpoint
ALTER TABLE "integrity_cases" ADD COLUMN "statistical" jsonb;--> statement-breakpoint
ALTER TABLE "integrity_rules" ADD COLUMN "assessment_mode" text DEFAULT 'statistical_shadow' NOT NULL;--> statement-breakpoint
ALTER TABLE "integrity_scores" ADD COLUMN "statistical" jsonb;--> statement-breakpoint
ALTER TABLE "integrity_windows" ADD COLUMN "max_kills_15s" integer;--> statement-breakpoint
ALTER TABLE "integrity_windows" ADD COLUMN "median_kill_interval" real;--> statement-breakpoint
ALTER TABLE "integrity_baselines" ADD CONSTRAINT "integrity_baselines_org_id_organizations_id_fk" FOREIGN KEY ("org_id") REFERENCES "public"."organizations"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
CREATE INDEX "integrity_baselines_lookup_idx" ON "integrity_baselines" USING btree ("org_id","metric","level");