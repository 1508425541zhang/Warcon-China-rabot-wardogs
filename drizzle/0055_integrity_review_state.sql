WITH latest AS (
 SELECT DISTINCT ON (case_id,org_id) case_id,org_id,reviewer_id,created_at FROM integrity_labels ORDER BY case_id,org_id,created_at DESC,id DESC
)
UPDATE integrity_cases c SET status='REVIEWED',reviewed_by=l.reviewer_id,reviewed_at=l.created_at
FROM latest l WHERE c.id=l.case_id AND c.org_id=l.org_id AND c.status='OPEN' AND c.reviewed_at IS NULL;
