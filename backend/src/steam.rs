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
pub async fn save(tx: &mut Transaction<'_, Postgres>, p: &Profile) -> anyhow::Result<()> {
    sqlx::query("INSERT INTO steam_profiles(steam_id,persona,avatar,profile_url,public,account_created_at,vac_bans,game_bans,days_since_last_ban,community_banned,economy_ban,error,fetched_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,now()) ON CONFLICT(steam_id) DO UPDATE SET persona=excluded.persona,avatar=excluded.avatar,profile_url=excluded.profile_url,public=excluded.public,account_created_at=excluded.account_created_at,vac_bans=excluded.vac_bans,game_bans=excluded.game_bans,days_since_last_ban=excluded.days_since_last_ban,community_banned=excluded.community_banned,economy_ban=excluded.economy_ban,error=excluded.error,fetched_at=excluded.fetched_at")
        .bind(&p.steam_id).bind(&p.persona).bind(&p.avatar).bind(&p.profile_url).bind(p.public).bind(p.created).bind(p.vac_bans).bind(p.game_bans).bind(p.days_since_last_ban).bind(p.community_banned).bind(&p.economy_ban).bind(&p.error).execute(&mut **tx).await?;
    Ok(())
}
pub async fn process_next(leader: &Leadership, client: &SteamClient) -> anyhow::Result<bool> {
    let mut tx = leader.transaction().await?;
    let job:Option<(String,i32,DateTime<Utc>)>=sqlx::query_as("UPDATE integrity_profile_refresh_jobs SET state='processing',attempts=attempts+1,lease_until=now()+interval '2 minutes' WHERE steam_id=(SELECT steam_id FROM integrity_profile_refresh_jobs WHERE next_at<=now() AND (state='pending' OR (state='processing' AND lease_until<=now())) AND EXISTS(SELECT 1 FROM integrity_scores s JOIN integrity_rules r ON r.org_id=s.org_id WHERE s.steam_id=integrity_profile_refresh_jobs.steam_id AND r.assessment_mode IN ('legacy','statistical','statistical_shadow')) ORDER BY next_at LIMIT 1 FOR UPDATE SKIP LOCKED) RETURNING steam_id,attempts,lease_until").fetch_optional(&mut *tx).await?;
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
