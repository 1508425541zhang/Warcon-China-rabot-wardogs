CREATE TABLE "integrity_windows" (
	"id" bigserial PRIMARY KEY NOT NULL,
	"org_id" text NOT NULL,
	"server_id" text NOT NULL,
	"steam_id" text NOT NULL,
	"instance_id" text NOT NULL,
	"map" text NOT NULL,
	"clock_from" real NOT NULL,
	"clock_to" real NOT NULL,
	"observed_at" timestamp with time zone NOT NULL,
	"infantry_kills" integer NOT NULL,
	"kpm_180" real NOT NULL,
	"unique_victims" integer NOT NULL,
	"event_ids" jsonb NOT NULL
);
--> statement-breakpoint
CREATE INDEX "integrity_windows_player_idx" ON "integrity_windows" USING btree ("org_id","steam_id","observed_at" DESC NULLS LAST);