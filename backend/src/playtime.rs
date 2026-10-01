use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
pub const APP_ID: i64 = 1867240;
pub fn parse_playtime(body: &Value) -> Option<i32> {
    let games = body.pointer("/response/games")?.as_array()?;
    let g = games
        .iter()
        .find(|g| g["appid"].as_f64() == Some(APP_ID as f64))?;
    let n = g["playtime_forever"].as_f64()?;
    (n.is_finite() && n.fract() == 0. && n >= 0. && n <= i32::MAX as f64).then_some(n as i32)
}
pub struct Group {
    pub minutes: Option<i64>,
    pub state: String,
    pub count: i64,
}
pub fn summarize(groups: &[Group], enabled: bool) -> Value {
    let mut known = groups
        .iter()
        .filter(|g| {
            g.state == "known" && g.minutes.is_some_and(|m| m >= 0 && m <= 9007199254740991)
        })
        .collect::<Vec<_>>();
    known.sort_by_key(|g| g.minutes);
    let total = groups.iter().map(|g| g.count).sum::<i64>();
    let available = known.iter().map(|g| g.count).sum::<i64>();
    let max_hours = known
        .last()
        .map(|g| g.minutes.unwrap() as f64 / 60.)
        .unwrap_or(0.);
    let width = [
        1., 2., 5., 10., 25., 50., 100., 250., 500., 1000., 2500., 5000., 10000., 100000.,
        1000000., 10000000.,
    ]
    .into_iter()
    .find(|w| max_hours / w < 20.)
    .unwrap_or(10000000.);
    // Stored minutes are int4; at most twenty bins for valid source data.
    let mut counts = if available > 0 {
        vec![0i64; (max_hours / width).floor() as usize + 1]
    } else {
        vec![]
    };
    let mut seen = 0;
    let mut p80 = None;
    for g in known {
        counts[(g.minutes.unwrap() as f64 / 60. / width).floor() as usize] += g.count;
        seen += g.count;
        if p80.is_none() && seen as f64 >= (available as f64 * 0.8).ceil() {
            p80 = g.minutes;
        }
    }
    seen = 0;
    let bins=counts.into_iter().enumerate().map(|(i,count)|{seen+=count;json!({"from":i as f64*width,"to":(i+1) as f64*width,"count":count,"percent":count as f64/available as f64*100.,"cumulative":seen as f64/available as f64*100.})}).collect::<Vec<_>>();
    json!({"enabled":enabled,"total":total,"available":available,"unknown":total-available,"p80Minutes":p80,"bins":bins,"pending":groups.iter().filter(|g|g.state=="pending").map(|g|g.count).sum::<i64>(),"errors":groups.iter().filter(|g|g.state=="error").map(|g|g.count).sum::<i64>()})
}
pub async fn distribution(
    db: &PgPool,
    id: &str,
    from: DateTime<Utc>,
    enabled: bool,
) -> anyhow::Result<Value> {
    let rows=sqlx::query("WITH cohort AS (SELECT DISTINCT steam_id FROM player_sessions WHERE server_id=$1 AND last_seen>=$2 AND steam_id ~ '^[0-9]{17}$'),samples AS (SELECT CASE WHEN p.checked_at>=now()-interval '24 hours' THEN p.minutes ELSE NULL END minutes,CASE WHEN p.state='error' THEN 'error' WHEN p.checked_at>=now()-interval '24 hours' THEN p.state ELSE 'pending' END state FROM cohort c LEFT JOIN steam_game_playtime p USING(steam_id)) SELECT minutes,state,count(*) count FROM samples GROUP BY minutes,state").bind(id).bind(from).fetch_all(db).await?;
    let mut groups = vec![];
    for row in rows {
        groups.push(Group {
            minutes: row.try_get::<Option<i32>, _>("minutes")?.map(i64::from),
            state: row
                .try_get::<Option<String>, _>("state")?
                .unwrap_or_else(|| "null".into()),
            count: row.try_get("count")?,
        });
    }
    Ok(summarize(&groups, enabled))
}
pub async fn refresh(
    leader: &crate::leadership::Leadership,
    steam: &crate::steam::SteamClient,
) -> anyhow::Result<usize> {
    if steam.backed_off().await {
        return Ok(0);
    }
    let mut tx = leader.transaction().await?;
    // Enqueuing is idempotent and uses a sixty-second database checkpoint shared by restarts.
    let due:bool=sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM site_settings WHERE key='rust:playtimeEnqueueAt' AND updated_at>now()-interval '60 seconds')").fetch_one(&mut *tx).await?;
    if due {
        sqlx::query("INSERT INTO steam_game_playtime(steam_id) SELECT DISTINCT steam_id FROM player_sessions WHERE last_seen>=now()-interval '30 days' AND steam_id ~ '^[0-9]{17}$' ON CONFLICT DO NOTHING").execute(&mut *tx).await?;
        sqlx::query("INSERT INTO site_settings(key,value,updated_at) VALUES('rust:playtimeEnqueueAt','true',now()) ON CONFLICT(key) DO UPDATE SET updated_at=excluded.updated_at").execute(&mut *tx).await?;
    }
    let claimed:Vec<(String,DateTime<Utc>)>=sqlx::query_as("UPDATE steam_game_playtime SET lease_until=now()+interval '1 minute' WHERE steam_id IN (SELECT p.steam_id FROM steam_game_playtime p WHERE next_at<=now() AND (lease_until IS NULL OR lease_until<now()) AND EXISTS(SELECT 1 FROM player_sessions s WHERE s.steam_id=p.steam_id AND s.last_seen>=now()-interval '30 days') ORDER BY next_at LIMIT 2 FOR UPDATE SKIP LOCKED) RETURNING steam_id,lease_until").fetch_all(&mut *tx).await?;
    tx.commit().await?;
    let size = claimed.len();
    for (id, lease) in claimed {
        let (minutes, state) = match steam.owned_games(&id).await {
            Ok(body) => {
                let minutes = parse_playtime(&body);
                (
                    minutes,
                    if minutes.is_some() {
                        "known"
                    } else {
                        "unavailable"
                    },
                )
            }
            Err(_) => (None, "error"),
        };
        let mut tx = leader.transaction().await?;
        sqlx::query("UPDATE steam_game_playtime SET minutes=$3,state=$4,checked_at=now(),next_at=now()+CASE WHEN $4='error' THEN interval '1 hour' ELSE interval '24 hours' END,lease_until=NULL WHERE steam_id=$1 AND lease_until=$2 AND lease_until>now()")
            .bind(&id).bind(lease).bind(minutes).bind(state).execute(&mut *tx).await?;
        tx.commit().await?;
    }
    Ok(size)
}
