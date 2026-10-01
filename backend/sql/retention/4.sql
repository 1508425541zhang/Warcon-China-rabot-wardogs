CREATE TEMP TABLE history_retention_runs ON COMMIT DROP AS
		 WITH ranked AS (SELECT id,created_at,state,action_state,row_number() OVER(ORDER BY created_at DESC,id DESC) AS ordinal
		 FROM integrity_model_runs WHERE server_id=$1 AND action IS NULL AND punished_at IS NULL)
		 SELECT id FROM ranked WHERE (ordinal>$2 OR created_at<now()-($3*interval '1 day'))
		 AND created_at<now()-interval '7 days' AND (state IN ('ERROR','superseded') OR (state='READY' AND action_state='skipped'))
		 ORDER BY created_at,id LIMIT 500
