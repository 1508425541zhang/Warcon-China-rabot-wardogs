CREATE TABLE training_feed_batches (
 id bigserial PRIMARY KEY,
 server_id text NOT NULL,
 received_at timestamptz NOT NULL,
 instance_id text NOT NULL,
 parser_version integer NOT NULL DEFAULT 1,
 payload jsonb NOT NULL,
 CONSTRAINT training_feed_envelope CHECK (jsonb_typeof(payload->'events') = 'array')
);
--> statement-breakpoint
CREATE INDEX training_feed_batches_server_time_idx ON training_feed_batches(server_id, received_at, id);
--> statement-breakpoint
CREATE TABLE training_observations (
 id bigserial PRIMARY KEY,
 server_id text NOT NULL,
 poll_started_at timestamptz NOT NULL,
 received_at timestamptz NOT NULL,
 endpoint text NOT NULL CHECK (endpoint IN ('/v1/players', '/v1/status')),
 payload jsonb NOT NULL
);
--> statement-breakpoint
CREATE INDEX training_observations_server_time_idx ON training_observations(server_id, received_at, id);
--> statement-breakpoint
-- No retention policy or cascading server deletion: these are training source records.
-- Expand all events, including unknown types and malformed individual entries. Keep repeated
-- deliveries; exporters deduplicate with (server_id, instance_id, event_id), not timestamp alone.
CREATE VIEW training_feed_events AS
SELECT b.id AS batch_id, b.server_id, b.instance_id, b.received_at, b.parser_version,
 e.ordinality AS event_index,
 e.value->>'eventId' AS event_id, e.value->>'type' AS event_type,
 e.value->>'matchId' AS source_match_id, e.value->>'mapName' AS source_map,
 e.value->>'killerSteamId' AS killer_steam_id, e.value->>'victimSteamId' AS victim_steam_id,
 e.value->>'cause' AS weapon,
 CASE WHEN jsonb_typeof(e.value->'eventTime')='number' THEN (e.value->>'eventTime')::numeric END AS event_time_seconds,
 CASE WHEN jsonb_typeof(e.value->'distance')='number' THEN (e.value->>'distance')::numeric END AS raw_distance_cm,
 CASE WHEN jsonb_typeof(e.value->'contextTags')='array' THEN EXISTS (
   SELECT 1 FROM jsonb_array_elements_text(e.value->'contextTags') tag(v)
   WHERE tag.v IN ('Meta.Progression.Context.Player.KillContext.Headshot','Meta.PlayerKillFlag.Player.Headshot','Headshot')
 ) END AS headshot,
 CASE WHEN jsonb_typeof(e.value->'contextTags')='array' THEN EXISTS (
   SELECT 1 FROM jsonb_array_elements_text(e.value->'contextTags') tag(v)
   WHERE tag.v IN ('Meta.Progression.Context.Player.KillContext.Penetration','Meta.PlayerKillFlag.Player.Penetration','Penetration')
 ) END AS penetration,
 e.value->'contextTags' AS context_tags,
 e.value AS raw_event
FROM training_feed_batches b CROSS JOIN LATERAL jsonb_array_elements(b.payload->'events') WITH ORDINALITY e(value, ordinality);
