CREATE TABLE "qq_integrity_notifications" (
  "outbox_id" bigint NOT NULL REFERENCES "outbox"("id") ON DELETE CASCADE,
  "server_id" text NOT NULL REFERENCES "servers"("id") ON DELETE CASCADE,
  "group_id" text NOT NULL,
  "self_id" text NOT NULL,
  "content" text NOT NULL,
  "state" text DEFAULT 'pending' NOT NULL,
  "outcome" text,
  "created_at" timestamp with time zone DEFAULT now() NOT NULL,
  "finished_at" timestamp with time zone,
  PRIMARY KEY ("outbox_id", "group_id")
);
--> statement-breakpoint
CREATE INDEX "qq_integrity_notifications_pending_idx" ON "qq_integrity_notifications" ("state", "created_at");
