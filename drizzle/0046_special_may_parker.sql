CREATE TABLE "integrity_import_batches" (
	"id" text PRIMARY KEY NOT NULL,
	"org_id" text NOT NULL,
	"source_server" text NOT NULL,
	"file_sha256" text NOT NULL,
	"status" text DEFAULT 'STAGED' NOT NULL,
	"row_count" integer NOT NULL,
	"first_event_at" timestamp with time zone NOT NULL,
	"last_event_at" timestamp with time zone NOT NULL,
	"staged_at" timestamp with time zone DEFAULT now() NOT NULL,
	"staged_by" text NOT NULL,
	"reviewed_at" timestamp with time zone,
	"reviewed_by" text
);
--> statement-breakpoint
CREATE TABLE "integrity_import_kills" (
	"batch_id" text NOT NULL,
	"org_id" text NOT NULL,
	"source_server" text NOT NULL,
	"event_id" text NOT NULL,
	"event_at" timestamp with time zone NOT NULL,
	"instance_id" text NOT NULL,
	"match_id" text NOT NULL,
	"event_time" real NOT NULL,
	"map" text NOT NULL,
	"killer_steam_id" text NOT NULL,
	"victim_steam_id" text NOT NULL,
	"killer_faction" text NOT NULL,
	"victim_faction" text NOT NULL,
	"cause" text NOT NULL,
	"distance_m" real,
	"headshot" boolean NOT NULL,
	"penetration" boolean NOT NULL,
	"player_count" integer
);
--> statement-breakpoint
ALTER TABLE "integrity_baselines" ADD COLUMN "source" text DEFAULT 'local' NOT NULL;--> statement-breakpoint
ALTER TABLE "integrity_import_batches" ADD CONSTRAINT "integrity_import_batches_org_id_organizations_id_fk" FOREIGN KEY ("org_id") REFERENCES "public"."organizations"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "integrity_import_kills" ADD CONSTRAINT "integrity_import_kills_batch_id_integrity_import_batches_id_fk" FOREIGN KEY ("batch_id") REFERENCES "public"."integrity_import_batches"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
CREATE INDEX "integrity_import_batches_org_idx" ON "integrity_import_batches" USING btree ("org_id","staged_at" DESC NULLS LAST);--> statement-breakpoint
CREATE UNIQUE INDEX "integrity_import_batches_hash_idx" ON "integrity_import_batches" USING btree ("org_id","file_sha256");--> statement-breakpoint
CREATE UNIQUE INDEX "integrity_import_kills_source_event_idx" ON "integrity_import_kills" USING btree ("org_id","source_server","event_id");--> statement-breakpoint
CREATE INDEX "integrity_import_kills_batch_idx" ON "integrity_import_kills" USING btree ("batch_id");--> statement-breakpoint
CREATE INDEX "integrity_import_kills_event_at_idx" ON "integrity_import_kills" USING btree ("org_id","event_at");