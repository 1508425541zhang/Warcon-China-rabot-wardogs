CREATE TABLE "integrity_profiles" (
	"org_id" text NOT NULL,
	"steam_id" text NOT NULL,
	"current_name" text NOT NULL,
	"aliases" jsonb NOT NULL,
	"first_seen" timestamp with time zone NOT NULL,
	"last_seen" timestamp with time zone NOT NULL,
	CONSTRAINT "integrity_profiles_org_id_steam_id_pk" PRIMARY KEY("org_id","steam_id")
);
--> statement-breakpoint
ALTER TABLE "integrity_profiles" ADD CONSTRAINT "integrity_profiles_org_id_organizations_id_fk" FOREIGN KEY ("org_id") REFERENCES "public"."organizations"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
CREATE INDEX "integrity_profiles_last_seen_idx" ON "integrity_profiles" USING btree ("org_id","last_seen" DESC NULLS LAST);
--> statement-breakpoint
-- Existing sessions already identify players by SteamID64. Materialize them once; stats and
-- server history continue to come from player_sessions and match_players.
INSERT INTO "integrity_profiles" ("org_id", "steam_id", "current_name", "aliases", "first_seen", "last_seen")
SELECT s."org_id", p."steam_id",
       (array_agg(p."name" ORDER BY p."last_seen" DESC, p."id" DESC))[1],
       jsonb_agg(DISTINCT p."name"), MIN(p."joined_at"), MAX(p."last_seen")
  FROM "player_sessions" p
  JOIN "servers" s ON s."id" = p."server_id"
 WHERE p."steam_id" ~ '^[0-9]{17}$' AND p."name" <> ''
 GROUP BY s."org_id", p."steam_id"
ON CONFLICT ("org_id", "steam_id") DO NOTHING;
