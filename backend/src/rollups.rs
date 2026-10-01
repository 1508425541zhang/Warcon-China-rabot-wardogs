//! Duration-weighted hourly rollups. No raw observation is deleted.
use crate::leadership::Leadership;
use chrono::{DateTime, Utc};
pub const MAX_COVER_S: i64 = 600;
pub async fn rollup(leader: &Leadership) -> anyhow::Result<()> {
    let mut tx = leader.transaction().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('warcon:sample-rollups',0))")
        .execute(&mut *tx)
        .await?;
    let (since,until):(Option<DateTime<Utc>>,DateTime<Utc>)=sqlx::query_as("SELECT coalesce((SELECT max(bucket)-interval '1 hour' FROM sample_rollups),(SELECT min(ts) FROM samples)),date_trunc('hour',now())").fetch_one(&mut *tx).await?;
    let Some(since) = since.filter(|s| *s < until) else {
        tx.commit().await?;
        return Ok(());
    };
    let covered = "SELECT server_id,ts,ok,player_count,max_players,map,least(600,extract(epoch FROM (coalesce(lead(ts) OVER(PARTITION BY server_id ORDER BY ts),now())-ts)),extract(epoch FROM (date_trunc('hour',ts)+interval '1 hour'-ts))) dur FROM samples WHERE ts>=$1";
    sqlx::query(&format!("WITH s AS ({covered}) INSERT INTO sample_rollups(server_id,bucket,samples,ok_samples,up_s,down_s,player_s,max_players,max_cap) SELECT server_id,date_trunc('hour',ts),count(*),count(*) FILTER(WHERE ok),coalesce(sum(dur) FILTER(WHERE ok),0),coalesce(sum(dur) FILTER(WHERE NOT ok),0),coalesce(sum(player_count*dur) FILTER(WHERE ok),0),max(player_count) FILTER(WHERE ok),max(max_players) FROM s WHERE ts<$2 GROUP BY server_id,date_trunc('hour',ts) ON CONFLICT(server_id,bucket) DO UPDATE SET samples=excluded.samples,ok_samples=excluded.ok_samples,up_s=excluded.up_s,down_s=excluded.down_s,player_s=excluded.player_s,max_players=excluded.max_players,max_cap=excluded.max_cap"))
        .bind(since).bind(until).execute(&mut *tx).await?;
    sqlx::query(&format!("WITH s AS ({covered}) INSERT INTO sample_map_rollups(server_id,bucket,map,secs) SELECT server_id,date_trunc('hour',ts),map,sum(dur) FROM s WHERE ts<$2 AND ok AND map IS NOT NULL AND map<>'' GROUP BY server_id,date_trunc('hour',ts),map ON CONFLICT(server_id,bucket,map) DO UPDATE SET secs=excluded.secs"))
        .bind(since).bind(until).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
