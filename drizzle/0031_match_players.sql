CREATE TABLE "match_players" (
	"match_id" bigint NOT NULL,
	"server_id" text NOT NULL,
	"steam_id" text NOT NULL,
	"name" text NOT NULL,
	"faction" text,
	"seconds" integer DEFAULT 0 NOT NULL,
	"kills" integer DEFAULT 0 NOT NULL,
	"deaths" integer DEFAULT 0 NOT NULL,
	"cash_delta" integer DEFAULT 0 NOT NULL,
	"headshots" integer DEFAULT 0 NOT NULL,
	"team_kills" integer DEFAULT 0 NOT NULL,
	"suicides" integer DEFAULT 0 NOT NULL,
	"vehicle_kills" integer DEFAULT 0 NOT NULL,
	"longest_m" real,
	"kill_streak" integer DEFAULT 0 NOT NULL,
	"death_streak" integer DEFAULT 0 NOT NULL,
	CONSTRAINT "match_players_match_id_steam_id_pk" PRIMARY KEY("match_id","steam_id")
);
--> statement-breakpoint
CREATE INDEX "match_players_server_idx" ON "match_players" USING btree ("server_id","match_id");--> statement-breakpoint
CREATE INDEX "match_players_steam_idx" ON "match_players" USING btree ("steam_id","match_id");