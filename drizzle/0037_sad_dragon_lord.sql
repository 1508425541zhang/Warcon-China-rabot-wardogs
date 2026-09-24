CREATE TABLE "integrity_rules" (
	"org_id" text PRIMARY KEY NOT NULL,
	"version" integer DEFAULT 1 NOT NULL,
	"config" jsonb NOT NULL,
	"updated_by" text,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "integrity_scores" (
	"id" bigserial PRIMARY KEY NOT NULL,
	"window_id" bigint NOT NULL,
	"org_id" text NOT NULL,
	"server_id" text NOT NULL,
	"steam_id" text NOT NULL,
	"scored_at" timestamp with time zone NOT NULL,
	"rule_version" integer NOT NULL,
	"score" integer NOT NULL,
	"level" text NOT NULL,
	"breakdown" jsonb NOT NULL,
	"current_behavior_anomaly" boolean NOT NULL
);
--> statement-breakpoint
ALTER TABLE "integrity_rules" ADD CONSTRAINT "integrity_rules_org_id_organizations_id_fk" FOREIGN KEY ("org_id") REFERENCES "public"."organizations"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
CREATE INDEX "integrity_scores_player_idx" ON "integrity_scores" USING btree ("org_id","steam_id","scored_at" DESC NULLS LAST);