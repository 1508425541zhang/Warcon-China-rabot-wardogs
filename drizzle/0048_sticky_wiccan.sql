CREATE TABLE "integrity_player_careers" (
	"org_id" text NOT NULL,
	"steam_id" text NOT NULL,
	"first_seen_at" timestamp with time zone NOT NULL,
	"last_seen_at" timestamp with time zone NOT NULL,
	"lifetime_valid_kills" integer,
	"lifetime_playtime_seconds" integer,
	"lifetime_windows" integer NOT NULL,
	"lifetime_matches" integer NOT NULL,
	"active_days" integer NOT NULL,
	"kpm_distribution" jsonb NOT NULL,
	"headshot_distribution" jsonb,
	"burst_distribution" jsonb NOT NULL,
	"recent_24h" jsonb,
	"recent_7d" jsonb,
	"recent_30d" jsonb,
	"ordered_kpm" jsonb NOT NULL,
	"model_version" text NOT NULL,
	"feature_version" text NOT NULL,
	"status" text DEFAULT 'READY' NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "integrity_player_careers_org_id_steam_id_pk" PRIMARY KEY("org_id","steam_id")
);
--> statement-breakpoint
ALTER TABLE "integrity_windows" ADD COLUMN "round_id" text;--> statement-breakpoint
ALTER TABLE "integrity_player_careers" ADD CONSTRAINT "integrity_player_careers_org_id_organizations_id_fk" FOREIGN KEY ("org_id") REFERENCES "public"."organizations"("id") ON DELETE cascade ON UPDATE no action;