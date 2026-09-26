-- 0047 defaulted pre-existing jobs to legacy. Give each retained historical
-- batch its missing Integrity consumer without replaying player actions.
INSERT INTO feed_processing_jobs
    (server_id, kill_ts, event_ids, created_at, consumer, state, attempts)
SELECT j.server_id, j.kill_ts, j.event_ids,
       LEAST(j.created_at, now() - interval '6 minutes'),
       'integrity', 'pending', 0
FROM feed_processing_jobs j
WHERE j.consumer = 'legacy'
  AND j.created_at >= now() - interval '30 days'
  AND jsonb_typeof(j.event_ids) = 'array'
  AND jsonb_array_length(j.event_ids) > 0
  AND NOT EXISTS (
      SELECT 1 FROM feed_processing_jobs other
      WHERE other.consumer = 'integrity'
        AND other.server_id = j.server_id AND other.kill_ts = j.kill_ts
        AND other.event_ids = j.event_ids
  )
  AND NOT EXISTS (
      SELECT 1 FROM jsonb_array_elements_text(j.event_ids) AS e(id)
      WHERE NOT EXISTS (
          SELECT 1 FROM kills k WHERE k.server_id = j.server_id
            AND k.ts = j.kill_ts AND k.event_id = e.id
      )
  );
