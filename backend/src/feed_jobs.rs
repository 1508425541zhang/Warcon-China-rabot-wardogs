//! Ordered independent feed lanes. Claiming a job never implies its business effects succeeded.
use crate::leadership::Leadership;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::Row;
#[derive(Clone, Copy)]
pub enum Consumer {
    Legacy,
    Integrity,
}
impl Consumer {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::Integrity => "integrity",
        }
    }
}
pub struct Job {
    pub id: i64,
    pub server_id: String,
    pub kill_ts: DateTime<Utc>,
    pub event_ids: Value,
    pub consumer: String,
    pub created_at: DateTime<Utc>,
    pub attempts: i32,
    pub lease_until: DateTime<Utc>,
}
pub async fn claim(
    leader: &Leadership,
    consumer: Consumer,
    only_id: Option<i64>,
) -> anyhow::Result<Option<Job>> {
    let mut tx = leader.transaction().await?;
    let row=sqlx::query("UPDATE feed_processing_jobs SET state='processing',attempts=attempts+1,lease_until=now()+interval '2 minutes' WHERE id=(SELECT j.id FROM feed_processing_jobs j WHERE j.consumer=$1 AND (j.consumer<>'integrity' OR j.created_at<=now()-interval '60 seconds' OR EXISTS(SELECT 1 FROM server_live live WHERE live.server_id=j.server_id AND live.players_at>j.kill_ts)) AND ($2::bigint IS NULL OR j.id=$2) AND ((j.state='pending' AND (j.lease_until IS NULL OR j.lease_until<=now())) OR (j.state='processing' AND j.lease_until<=now())) AND NOT EXISTS(SELECT 1 FROM feed_processing_jobs earlier WHERE earlier.consumer=j.consumer AND earlier.server_id=j.server_id AND earlier.state<>'done' AND (earlier.created_at,earlier.id)<(j.created_at,j.id)) ORDER BY j.created_at,j.id LIMIT 1 FOR UPDATE OF j SKIP LOCKED) RETURNING id,server_id,kill_ts,event_ids,consumer,created_at,attempts,lease_until").bind(consumer.as_str()).bind(only_id).fetch_optional(&mut *tx).await?;
    let job = if let Some(row) = row {
        Some(Job {
            id: row.try_get("id")?,
            server_id: row.try_get("server_id")?,
            kill_ts: row.try_get("kill_ts")?,
            event_ids: row.try_get("event_ids")?,
            consumer: row.try_get("consumer")?,
            created_at: row.try_get("created_at")?,
            attempts: row.try_get("attempts")?,
            lease_until: row.try_get("lease_until")?,
        })
    } else {
        None
    };
    tx.commit().await?;
    Ok(job)
}
/// `error` must be a stable internal code, never a credential-bearing HTTP/SQL error string.
pub async fn finish(leader: &Leadership, job: &Job, error: Option<&str>) -> anyhow::Result<()> {
    let mut tx = leader.transaction().await?;
    let result = if let Some(error) = error {
        anyhow::ensure!(
            error.len() <= 100 && error.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "Invalid job error code"
        );
        sqlx::query("UPDATE feed_processing_jobs SET state='pending',lease_until=now()+interval '5 seconds',last_error=$4 WHERE id=$1 AND attempts=$2 AND lease_until=$3 AND state='processing' AND lease_until>now()").bind(job.id).bind(job.attempts).bind(job.lease_until).bind(error).execute(&mut *tx).await?
    } else {
        sqlx::query("UPDATE feed_processing_jobs SET state='done',lease_until=NULL,last_error=NULL,done_at=now() WHERE id=$1 AND attempts=$2 AND lease_until=$3 AND state='processing' AND lease_until>now()").bind(job.id).bind(job.attempts).bind(job.lease_until).execute(&mut *tx).await?
    };
    anyhow::ensure!(result.rows_affected() == 1, "Feed job lease lost");
    tx.commit().await?;
    Ok(())
}
pub async fn backlog_safe(leader: &Leadership, consumer: Consumer) -> anyhow::Result<bool> {
    let mut tx = leader.transaction().await?;
    let (pending,oldest):(i64,Option<f64>)=sqlx::query_as("SELECT count(*) FILTER(WHERE state='pending'),extract(epoch FROM (now()-min(created_at)))::double precision FROM feed_processing_jobs WHERE state IN ('pending','processing') AND consumer=$1").bind(consumer.as_str()).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(pending < 500 && oldest.is_none_or(|seconds| seconds <= 300.))
}
