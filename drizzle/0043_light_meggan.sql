ALTER TABLE "integrity_actions" ADD COLUMN "effective_at" timestamp with time zone;--> statement-breakpoint
ALTER TABLE "integrity_actions" ADD COLUMN "delivery_state" text;
--> statement-breakpoint
-- Old queued kicks are not assumed successful. Only a confirmed delivered outbox row
-- can establish their effective time; quarantine was effective when its list entry committed.
UPDATE "integrity_actions" AS a
SET "delivery_state" = COALESCE((
  SELECT CASE WHEN o."state" IN ('delivered', 'failed', 'skipped', 'unknown')
              THEN o."state" ELSE 'unknown' END
  FROM "outbox" AS o
  WHERE o."trigger_kind" = 'integrity' AND o."detail"->>'actionId' = a."id"
  ORDER BY o."id" DESC LIMIT 1
), 'unknown'),
"effective_at" = CASE
  WHEN a."action" IN ('QUARANTINE_24H', 'QUARANTINE_7D')
       AND EXISTS (SELECT 1 FROM "list_entries" AS e WHERE e."id" = a."list_entry_id")
    THEN a."created_at"
  WHEN a."action" = 'KICK' THEN (
    SELECT o."done_at" FROM "outbox" AS o
    WHERE o."trigger_kind" = 'integrity' AND o."detail"->>'actionId' = a."id"
      AND o."state" = 'delivered'
    ORDER BY o."id" DESC LIMIT 1
  )
  ELSE NULL
END;
