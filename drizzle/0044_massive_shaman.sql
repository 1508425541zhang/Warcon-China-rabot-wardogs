CREATE TABLE "feed_processing_jobs" (
	"id" bigserial PRIMARY KEY NOT NULL,
	"server_id" text NOT NULL,
	"kill_ts" timestamp with time zone NOT NULL,
	"event_ids" jsonb NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"state" text DEFAULT 'pending' NOT NULL,
	"attempts" integer DEFAULT 0 NOT NULL,
	"lease_until" timestamp with time zone,
	"done_at" timestamp with time zone,
	"last_error" text
);
--> statement-breakpoint
ALTER TABLE "feed_processing_jobs" ADD CONSTRAINT "feed_processing_jobs_server_id_servers_id_fk" FOREIGN KEY ("server_id") REFERENCES "public"."servers"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
CREATE INDEX "feed_processing_pending_idx" ON "feed_processing_jobs" USING btree ("state","created_at");