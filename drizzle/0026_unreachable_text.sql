-- Error text stored before the message stopped naming the target said "Could not reach host:port",
-- and for a while after that it could still quote the port in a URL. These columns are read by
-- everyone who can open the server (and the audit trail, and the automation page), so what is
-- already stored is rewritten to the plain sentence. samples.error is never read out and is left.
UPDATE "audit_log" SET "message" = 'Could not reach the game server.'
 WHERE "message" LIKE '%Could not reach %'
   AND "message" NOT IN ('Could not reach the game server.', 'Could not reach the server.')
   AND "message" NOT LIKE 'Could not reach Discord%';--> statement-breakpoint
UPDATE "outbox" SET "outcome" = 'Could not reach the game server.'
 WHERE "outcome" LIKE '%Could not reach %'
   AND "outcome" NOT IN ('Could not reach the game server.', 'Could not reach the server.')
   AND "outcome" NOT LIKE 'Could not reach Discord%';--> statement-breakpoint
UPDATE "triggers" SET "last_result" = 'Could not reach the game server.'
 WHERE "last_result" LIKE '%Could not reach %'
   AND "last_result" NOT IN ('Could not reach the game server.', 'Could not reach the server.')
   AND "last_result" NOT LIKE 'Could not reach Discord%';--> statement-breakpoint
UPDATE "server_list_sync" SET "last_error" = 'Could not reach the game server.'
 WHERE "last_error" LIKE '%Could not reach %'
   AND "last_error" NOT IN ('Could not reach the game server.', 'Could not reach the server.')
   AND "last_error" NOT LIKE 'Could not reach Discord%';--> statement-breakpoint
UPDATE "server_list_state" SET "error" = 'Could not reach the game server.'
 WHERE "error" LIKE '%Could not reach %'
   AND "error" NOT IN ('Could not reach the game server.', 'Could not reach the server.')
   AND "error" NOT LIKE 'Could not reach Discord%';--> statement-breakpoint
UPDATE "server_live" SET "error" = 'Could not reach the game server.'
 WHERE "error" LIKE '%Could not reach %'
   AND "error" NOT IN ('Could not reach the game server.', 'Could not reach the server.')
   AND "error" NOT LIKE 'Could not reach Discord%';
