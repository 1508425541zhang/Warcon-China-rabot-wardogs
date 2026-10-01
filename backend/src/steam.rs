//! Independent Steam enrichment. An ordered feed consumer never waits for this client.
use crate::leadership::Leadership;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

#[derive(Debug)]
pub struct Profile {
    pub steam_id: String,
    pub persona: String,
    pub avatar: String,
    pub profile_url: String,
    pub public: bool,
    pub created: Option<DateTime<Utc>>,
    pub vac_bans: i32,
    pub game_bans: i32,
    pub days_since_last_ban: Option<i32>,
    pub community_banned: bool,
    pub economy_ban: String,
    pub error: String,
}
pub struct SteamClient {
    key: String,
    base: url::Url,
    client: reqwest::Client,
    backoff: Mutex<Option<Instant>>,
}
impl SteamClient {
    pub async fn friends(&self, id: &str) -> anyhow::Result<Value> {
        anyhow::ensure!(
            id.len() == 17 && id.bytes().all(|b| b.is_ascii_digit()),
            "invalid_steam_id"
        );
        if self.backed_off().await {
            return Ok(serde_json::json!({"state":"unknown","total":0,"checked":0,"banned":0}));
        }
        let mut url = self.base.join("ISteamUser/GetFriendList/v1/")?;
        url.query_pairs_mut()
            .append_pair("key", &self.key)
            .append_pair("steamid", id)
            .append_pair("relationship", "friend");
        let response = self
            .client
            .get(url)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("steam_unreachable"))?;
        if response.status().as_u16() == 401 {
            return Ok(serde_json::json!({"state":"private","total":0,"checked":0,"banned":0}));
        }
        if response.status().as_u16() == 429 || response.status().is_server_error() {
            *self.backoff.lock().await = Some(Instant::now() + Duration::from_secs(60));
        }
        anyhow::ensure!(response.status().is_success(), "steam_friends_unavailable");
        let mut response = response;
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| anyhow::anyhow!("steam_response"))?
        {
            anyhow::ensure!(
                bytes.len() + chunk.len() <= 2 * 1024 * 1024,
                "steam_response_size"
            );
            bytes.extend(chunk)
        }
        let body: Value =
            serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("steam_response"))?;
        anyhow::ensure!(body["friendslist"].is_object(), "steam_friends_unavailable");
        let mut seen = std::collections::HashSet::new();
        let friends = body["friendslist"]["friends"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p["steamid"].as_str())
            .filter(|id| {
                id.len() == 17
                    && id.bytes().all(|b| b.is_ascii_digit())
                    && seen.insert(id.to_string())
            })
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let sample = &friends[..friends.len().min(200)];
        let mut banned = 0;
        for ids in sample.chunks(100) {
            let bans = self
                .get("ISteamUser/GetPlayerBans/v1/", &ids.join(","))
                .await?;
            banned += bans["players"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|p| {
                    p["NumberOfVACBans"].as_i64().unwrap_or(0) > 0
                        || p["NumberOfGameBans"].as_i64().unwrap_or(0) > 0
                })
                .count();
        }
        Ok(
            serde_json::json!({"state":if sample.len()<friends.len(){"partial"}else{"public"},"total":friends.len(),"checked":sample.len(),"banned":banned}),
        )
    }
    pub async fn summaries(&self, ids: &[String]) -> anyhow::Result<Value> {
        anyhow::ensure!(
            ids.len() <= 100
                && ids
                    .iter()
                    .all(|s| s.len() == 17 && s.bytes().all(|b| b.is_ascii_digit())),
            "invalid_steam_ids"
        );
        self.get("ISteamUser/GetPlayerSummaries/v2/", &ids.join(","))
            .await
    }
    pub fn new(key: String) -> anyhow::Result<Self> {
        Self::with_endpoint(key, url::Url::parse("https://api.steampowered.com/")?)
    }
    /// Test injection is deliberately limited to loopback HTTP fixtures.
    pub fn loopback_fixture(key: String, base: url::Url) -> anyhow::Result<Self> {
        anyhow::ensure!(
            base.scheme() == "http"
                && base.host_str().is_some_and(|h| h
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())),
            "Fixture endpoint must be loopback"
        );
        Self::with_endpoint(key, base)
    }
    fn with_endpoint(key: String, base: url::Url) -> anyhow::Result<Self> {
        anyhow::ensure!(!key.trim().is_empty(), "STEAM_API_KEY is required");
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(8))
            .build()?;
        Ok(Self {
            key,
            base,
            client,
            backoff: Mutex::new(None),
        })
    }
    async fn get(&self, path: &str, id: &str) -> anyhow::Result<Value> {
        let mut url = self.base.join(path)?;
        url.query_pairs_mut()
            .append_pair("key", &self.key)
            .append_pair("steamids", id);
        self.get_url(url, false).await
    }
    pub async fn owned_games(&self, id: &str) -> anyhow::Result<Value> {
        anyhow::ensure!(
            id.len() == 17 && id.bytes().all(|b| b.is_ascii_digit()),
            "invalid_steam_id"
        );
        let mut url = self.base.join("IPlayerService/GetOwnedGames/v1/")?;
        url.query_pairs_mut().append_pair("key",&self.key).append_pair("input_json",&serde_json::json!({"steamid":id,"appids_filter":[crate::playtime::APP_ID],"include_appinfo":false,"include_played_free_games":true}).to_string());
        self.get_url(url, true).await
    }
    pub async fn backed_off(&self) -> bool {
        self.backoff
            .lock()
            .await
            .is_some_and(|until| until > Instant::now())
    }
    async fn get_url(&self, url: url::Url, playtime: bool) -> anyhow::Result<Value> {
        if self
            .backoff
            .lock()
            .await
            .is_some_and(|until| until > Instant::now())
        {
            anyhow::bail!("steam_backoff")
        }
        // Never return reqwest's error display: it contains the key in its URL.
        let response = match self
            .client
            .get(url)
            .timeout(Duration::from_secs(if playtime { 5 } else { 8 }))
            .send()
            .await
        {
            Ok(r) => r,
            Err(_) => {
                *self.backoff.lock().await = Some(Instant::now() + Duration::from_secs(60));
                anyhow::bail!("steam_unreachable")
            }
        };
        let status = response.status();
        if status.as_u16() == 429 || status.is_server_error() {
            *self.backoff.lock().await =
                Some(Instant::now() + Duration::from_secs(if playtime { 300 } else { 60 }));
            anyhow::bail!("steam_error")
        }
        if status.as_u16() == 401 || status.as_u16() == 403 {
            if playtime {
                *self.backoff.lock().await = Some(Instant::now() + Duration::from_secs(300));
            }
            anyhow::bail!("steam_key")
        }
        anyhow::ensure!(status.is_success(), "steam_error");
        let mut response = response;
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| anyhow::anyhow!("steam_response"))?
        {
            anyhow::ensure!(
                bytes.len() + chunk.len() <= 2 * 1024 * 1024,
                "steam_response_size"
            );
            bytes.extend_from_slice(&chunk)
        }
        serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("steam_response"))
    }
    pub async fn profile(&self, id: &str) -> anyhow::Result<Profile> {
        anyhow::ensure!(
            id.len() == 17 && id.bytes().all(|c| c.is_ascii_digit()),
            "invalid_steam_id"
        );
        let (summaries, bans) = tokio::try_join!(
            self.get("ISteamUser/GetPlayerSummaries/v2/", id),
            self.get("ISteamUser/GetPlayerBans/v1/", id)
        )?;
        let s = summaries
            .pointer("/response/players")
            .and_then(Value::as_array)
            .and_then(|rows| rows.iter().find(|r| r["steamid"].as_str() == Some(id)));
        let b = bans["players"]
            .as_array()
            .and_then(|rows| rows.iter().find(|r| r["SteamId"].as_str() == Some(id)));
        let string = |v: Option<&Value>, name: &str| {
            v.and_then(|v| v[name].as_str()).unwrap_or("").to_owned()
        };
        let number = |name: &str| {
            b.and_then(|b| b[name].as_i64())
                .unwrap_or(0)
                .clamp(0, i32::MAX as i64) as i32
        };
        let vac_bans = number("NumberOfVACBans");
        let game_bans = number("NumberOfGameBans");
        Ok(Profile {
            steam_id: id.into(),
            persona: string(s, "personaname"),
            avatar: if string(s, "avatarmedium").is_empty() {
                string(s, "avatar")
            } else {
                string(s, "avatarmedium")
            },
            profile_url: string(s, "profileurl"),
            public: s.is_some_and(|s| s["communityvisibilitystate"].as_i64() == Some(3)),
            created: s
                .and_then(|s| s["timecreated"].as_i64())
                .filter(|v| *v > 0)
                .and_then(|t| DateTime::from_timestamp(t, 0)),
            vac_bans,
            game_bans,
            days_since_last_ban: if vac_bans > 0 || game_bans > 0 {
                b.and_then(|b| b["DaysSinceLastBan"].as_i64())
                    .map(|n| n.clamp(0, i32::MAX as i64) as i32)
            } else {
                None
            },
            community_banned: b.is_some_and(|b| b["CommunityBanned"].as_bool() == Some(true)),
            economy_ban: if string(b, "EconomyBan").is_empty() {
                "none".into()
            } else {
                string(b, "EconomyBan")
            },
            error: if s.is_none() {
                "Not found on Steam.".into()
            } else if b.is_none() {
                "Steam did not return ban data.".into()
            } else {
                String::new()
            },
        })
    }
}
pub async fn refresh_friends(
    state: &crate::config::AppState,
    client: &SteamClient,
    id: &str,
) -> crate::error::Result<()> {
    let old: Option<(String, Option<DateTime<Utc>>, String)> = sqlx::query_as(
        "SELECT friends_state,friends_checked_at,error FROM steam_profiles WHERE steam_id=$1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?;
    let Some((kind, at, error)) = old.filter(|r| r.2.is_empty()) else {
        return Ok(());
    };
    let _ = error;
    if at.is_some_and(|at| {
        (Utc::now() - at).num_seconds() < if kind == "unknown" { 3600 } else { 7 * 86400 }
    }) {
        return Ok(());
    }
    let mut tx = state.db.begin().await?;
    let day = Utc::now().timestamp().div_euclid(86400);
    let budget = format!("rust:steamFriendBudget:{day}");
    let granted:Option<Value>=sqlx::query_scalar("INSERT INTO site_settings(key,value)VALUES($1,'3'::jsonb)ON CONFLICT(key)DO UPDATE SET value=to_jsonb((site_settings.value#>>'{}')::int+3)WHERE(site_settings.value#>>'{}')::int+3<=20000 RETURNING value").bind(&budget).fetch_optional(&mut *tx).await?;
    tx.commit().await?;
    if granted.is_none() {
        return Ok(());
    }
    let result = client
        .friends(id)
        .await
        .unwrap_or(serde_json::json!({"state":"unknown","total":0,"checked":0,"banned":0}));
    let mut tx = if state.runtime.leader.get().is_some() {
        state.worker_transaction().await?
    } else {
        state.db.begin().await?
    };
    if result["state"] == "unknown" {
        if kind == "unknown" {
            sqlx::query("UPDATE steam_profiles SET friends_checked_at=now()WHERE steam_id=$1 AND friends_state='unknown' AND friends_checked_at IS NOT DISTINCT FROM $2").bind(id).bind(at).execute(&mut *tx).await?;
        }
    } else {
        sqlx::query("UPDATE steam_profiles SET friends_state=$2,friends_total=$3,friends_checked=$4,banned_friends=$5,friends_checked_at=now()WHERE steam_id=$1 AND friends_checked_at IS NOT DISTINCT FROM $6").bind(id).bind(result["state"].as_str()).bind(result["total"].as_i64().unwrap_or(0)as i32).bind(result["checked"].as_i64().unwrap_or(0)as i32).bind(result["banned"].as_i64().unwrap_or(0)as i32).bind(at).execute(&mut *tx).await?;
    }
    sqlx::query("DELETE FROM site_settings WHERE key LIKE 'rust:steamFriendBudget:%' AND key<>$1 AND updated_at<now()-interval '2 days'").bind(&budget).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
pub async fn friends_pass(
    state: &crate::config::AppState,
    client: &SteamClient,
) -> crate::error::Result<()> {
    state.runtime.check().await?;
    let ids:Vec<String>=sqlx::query_scalar("SELECT p.steam_id FROM steam_profiles p WHERE p.error='' AND(p.friends_checked_at IS NULL OR p.friends_checked_at<now()-CASE WHEN p.friends_state='unknown' THEN interval '1 hour' ELSE interval '7 days' END)AND(EXISTS(SELECT 1 FROM player_sessions ps JOIN servers s ON s.id=ps.server_id JOIN organizations o ON o.id=s.org_id WHERE ps.steam_id=p.steam_id AND ps.left_at IS NULL AND o.suspended_at IS NULL)OR EXISTS(SELECT 1 FROM integrity_profile_refresh_jobs j WHERE j.steam_id=p.steam_id AND j.updated_at>now()-interval '1 day'))ORDER BY p.friends_checked_at NULLS FIRST LIMIT 8").fetch_all(&state.db).await?;
    let jobs = ids.iter().map(|id| refresh_friends(state, client, id));
    let results = futures_util::future::join_all(jobs).await;
    for r in results {
        r?
    }
    Ok(())
}
pub async fn request_refresh(
    state: &crate::config::AppState,
    ids: &[String],
) -> crate::error::Result<()> {
    if std::env::var("STEAM_API_KEY").is_ok_and(|s| !s.is_empty()) {
        let mut tx = state.db.begin().await?;
        enqueue(&mut tx, ids).await?;
        tx.commit().await?;
    }
    Ok(())
}
pub async fn save(tx: &mut Transaction<'_, Postgres>, p: &Profile) -> anyhow::Result<()> {
    sqlx::query("INSERT INTO steam_profiles(steam_id,persona,avatar,profile_url,public,account_created_at,vac_bans,game_bans,days_since_last_ban,community_banned,economy_ban,error,fetched_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,now()) ON CONFLICT(steam_id) DO UPDATE SET persona=excluded.persona,avatar=excluded.avatar,profile_url=excluded.profile_url,public=excluded.public,account_created_at=excluded.account_created_at,vac_bans=excluded.vac_bans,game_bans=excluded.game_bans,days_since_last_ban=excluded.days_since_last_ban,community_banned=excluded.community_banned,economy_ban=excluded.economy_ban,error=excluded.error,fetched_at=excluded.fetched_at")
        .bind(&p.steam_id).bind(&p.persona).bind(&p.avatar).bind(&p.profile_url).bind(p.public).bind(p.created).bind(p.vac_bans).bind(p.game_bans).bind(p.days_since_last_ban).bind(p.community_banned).bind(&p.economy_ban).bind(&p.error).execute(&mut **tx).await?;
    Ok(())
}
pub async fn enqueue(
    tx: &mut Transaction<'_, Postgres>,
    ids: &[String],
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO integrity_profile_refresh_jobs(steam_id,state,next_at) SELECT DISTINCT id,'pending',now() FROM unnest($1::text[]) AS id WHERE NOT EXISTS(SELECT 1 FROM steam_profiles p WHERE p.steam_id=id AND p.fetched_at>now()-interval '24 hours') ON CONFLICT(steam_id) DO UPDATE SET state='pending',next_at=now(),lease_until=NULL,attempts=0 WHERE integrity_profile_refresh_jobs.state NOT IN ('pending','processing')").bind(ids).execute(&mut **tx).await?;
    Ok(())
}
pub async fn process_next(leader: &Leadership, client: &SteamClient) -> anyhow::Result<bool> {
    let mut tx = leader.transaction().await?;
    let job:Option<(String,i32,DateTime<Utc>)>=sqlx::query_as("UPDATE integrity_profile_refresh_jobs SET state='processing',attempts=attempts+1,lease_until=now()+interval '2 minutes' WHERE steam_id=(SELECT steam_id FROM integrity_profile_refresh_jobs j WHERE next_at<=now() AND (state='pending' OR (state='processing' AND lease_until<=now())) ORDER BY next_at LIMIT 1 FOR UPDATE SKIP LOCKED) RETURNING steam_id,attempts,lease_until").fetch_optional(&mut *tx).await?;
    tx.commit().await?;
    let Some((id, attempt, lease)) = job else {
        return Ok(false);
    };
    let result = client.profile(&id).await;
    // The network request holds no DB lock. Recheck both worker and job leases before saving.
    let mut tx = leader.transaction().await?;
    let owned:Option<bool>=sqlx::query_scalar("SELECT state='processing' AND attempts=$2 AND lease_until=$3 AND lease_until>now() FROM integrity_profile_refresh_jobs WHERE steam_id=$1 FOR UPDATE").bind(&id).bind(attempt).bind(lease).fetch_optional(&mut *tx).await?;
    anyhow::ensure!(owned == Some(true), "profile_job_lease_lost");
    match result {
        Ok(profile) => {
            save(&mut tx, &profile).await?;
            sqlx::query("UPDATE integrity_profile_refresh_jobs SET state='done',lease_until=NULL,last_error=NULL,updated_at=now() WHERE steam_id=$1").bind(&id).execute(&mut *tx).await?;
        }
        Err(error) => {
            let delay = (30_000i64 * (1i64 << attempt.clamp(0, 10))).min(86_400_000);
            sqlx::query("UPDATE integrity_profile_refresh_jobs SET state='pending',lease_until=NULL,next_at=now()+($2::bigint*interval '1 millisecond'),last_error=$3,updated_at=now() WHERE steam_id=$1").bind(&id).bind(delay).bind(error.to_string().chars().take(500).collect::<String>()).execute(&mut *tx).await?;
        }
    }
    tx.commit().await?;
    Ok(true)
}
