//! Native game client. Calls require a held dispatcher lane and a freshly validated target.
use crate::{
    config::AppState,
    crypto,
    error::ApiError,
    rcon::{self, GameRequest, GameResponse, GameTarget},
};
use serde_json::{Value, json};
use std::collections::HashMap;
#[derive(Debug)]
pub struct GameError {
    pub status: u16,
    pub code: String,
    pub message: String,
    pub body: Value,
    pub retry_after_ms: u64,
}
#[derive(Debug)]
pub enum Error {
    Api(ApiError),
    Game(GameError),
}
impl From<ApiError> for Error {
    fn from(e: ApiError) -> Self {
        Self::Api(e)
    }
}
impl From<sqlx::Error> for Error {
    fn from(e: sqlx::Error) -> Self {
        Self::Api(e.into())
    }
}
pub type Result<T> = std::result::Result<T, Error>;
impl Error {
    pub fn unavailable() -> Self {
        Self::Game(GameError {
            status: 502,
            code: "unreachable".into(),
            message: "Could not reach the game server.".into(),
            body: Value::Null,
            retry_after_ms: 0,
        })
    }
}
pub fn classify(method: &str, path: &str, response: &GameResponse, body: Value) -> GameError {
    let mut code = body["error"]["code"].as_str().unwrap_or("").to_owned();
    let upstream = body["error"]["message"].as_str().unwrap_or("");
    let mut message = if upstream.is_empty() {
        format!(
            "Server answered {} {}.",
            response.status, response.status_text
        )
    } else {
        crate::feed::truncate(upstream.into(), 2000)
    };
    let mut retry_after_ms = 0;
    if response.status == 429 {
        retry_after_ms = retry_after(
            response.headers.get("retry-after").map(String::as_str),
            chrono::Utc::now(),
        );
        code = "rate_limited".into();
        message = format!(
            "The game server is rate limiting this panel; retry in {} s.",
            retry_after_ms.div_ceil(1000)
        );
    }
    if response.status == 404
        && code == "not_found"
        && upstream.to_lowercase().contains("no such endpoint")
    {
        code = "no_route".into();
        message = format!(
            "This server build does not serve {method} {}.",
            path.split('?').next().unwrap_or(path)
        );
    }
    if response.status == 405 {
        message = format!(
            "This server build does not serve {method} {}.",
            path.split('?').next().unwrap_or(path)
        );
        if code.is_empty() {
            code = "method_not_allowed".into()
        }
    }
    GameError {
        status: response.status,
        code,
        message,
        body,
        retry_after_ms,
    }
}
pub fn retry_after(value: Option<&str>, now: chrono::DateTime<chrono::Utc>) -> u64 {
    let value = value.unwrap_or("").trim();
    let ms = if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) {
        value.parse::<i64>().ok().and_then(|n| n.checked_mul(1000))
    } else {
        chrono::DateTime::parse_from_rfc2822(value)
            .ok()
            .map(|d| (d.to_utc() - now).num_milliseconds())
    };
    ms.unwrap_or(5000).clamp(1000, 60000) as u64
}
pub fn etag(headers: &HashMap<String, String>) -> String {
    headers
        .get("etag")
        .map(|s| {
            s.strip_prefix("W/")
                .unwrap_or(s)
                .replace('"', "")
                .trim()
                .into()
        })
        .unwrap_or_default()
}
pub struct Client {
    pub state: AppState,
    pub server: String,
    target: GameTarget,
    key: String,
    insecure: bool,
    demo: bool,
}
impl Client {
    pub async fn for_server(state: &AppState, id: &str) -> Result<Self> {
        let row: Option<(String, i32, String, String, bool)> = sqlx::query_as(
            "SELECT s.host,s.port,s.scheme,s.password_enc,s.allow_private FROM servers s JOIN organizations o ON o.id=s.org_id WHERE s.id=$1 AND o.suspended_at IS NULL",
        )
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
        let (host, port, scheme, blob, allow_private) = row.ok_or_else(ApiError::missing)?;
        let port = u16::try_from(port).map_err(|_| ApiError::bad("Invalid RCON port."))?;
        let demo = crate::mockgame::is_demo(&host);
        let target = if demo {
            GameTarget {
                host: "demo".into(),
                port,
                scheme,
                addresses: None,
            }
        } else {
            rcon::resolve_target(&host, port, &scheme, allow_private)
                .await
                .map_err(|_| ApiError::bad("Game server target is not permitted."))?
        };
        let key = crypto::decrypt_secret(&state.config.encryption_key, &blob)
            .map_err(|_| ApiError::bad("Could not decrypt the stored RCON password."))?;
        let insecure = std::env::var("GAME_TLS_INSECURE").is_ok_and(|v| v == "true" || v == "1");
        Ok(Self {
            state: state.clone(),
            server: id.into(),
            target,
            key,
            insecure,
            demo,
        })
    }
    pub async fn raw(
        &self,
        method: &str,
        path: &str,
        body: Option<String>,
        mut headers: HashMap<String, String>,
    ) -> Result<GameResponse> {
        self.state.runtime.check().await?;
        // A permit precedes every panel/balancer faction move, including raw actions.
        if method == "PATCH" {
            let parts: Vec<_> = path.split('/').collect();
            if parts.len() == 4
                && parts[1] == "v1"
                && parts[2] == "players"
                && parts[3].len() == 17
                && parts[3].bytes().all(|b| b.is_ascii_digit())
            {
                if let Some(faction) = body
                    .as_deref()
                    .and_then(|b| serde_json::from_str::<Value>(b).ok())
                    .and_then(|v| v["faction"].as_str().map(str::to_owned))
                    .filter(|s| !s.is_empty() && s.encode_utf16().count() <= 100)
                {
                    let mut tx = self.state.worker_transaction().await?;
                    sqlx::query("INSERT INTO faction_move_permits(server_id,steam_id,faction,expires_at) VALUES($1,$2,$3,now()+interval '120 seconds')").bind(&self.server).bind(parts[3]).bind(faction).execute(&mut *tx).await?;
                    tx.commit().await?;
                }
            }
        }
        headers.insert("authorization".into(), format!("Bearer {}", self.key));
        if self.demo {
            return Ok(crate::mockgame::request(
                &self.state.runtime,
                &self.server,
                &self.key,
                method,
                path,
                &headers,
                body.as_deref(),
            ));
        }
        rcon::game_request(
            &self.target,
            &GameRequest {
                method: method.into(),
                path: path.into(),
                headers,
                body,
                timeout_ms: Some(10000),
                insecure_tls: self.insecure,
            },
        )
        .await
        .map_err(|_| Error::unavailable())
    }
    pub async fn json(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value> {
        let mut headers = HashMap::new();
        if body.is_some() {
            headers.insert("content-type".into(), "application/json".into());
        }
        let response = self
            .raw(method, path, body.map(|b| b.to_string()), headers)
            .await?;
        let parsed = serde_json::from_str(&response.text).unwrap_or_else(|_| json!({}));
        if !(200..300).contains(&response.status) {
            return Err(Error::Game(classify(method, path, &response, parsed)));
        }
        Ok(parsed)
    }
    pub async fn config_call(
        &self,
        method: &str,
        path: &str,
        text: String,
        revision: Option<&str>,
    ) -> Result<(u16, Value, String)> {
        let mut headers = HashMap::from([("content-type".into(), "text/plain".into())]);
        if let Some(revision) = revision.filter(|s| !s.is_empty()) {
            headers.insert("if-match".into(), format!("\"{revision}\""));
        }
        let response = self.raw(method, path, Some(text), headers).await?;
        Ok((
            response.status,
            serde_json::from_str(&response.text).unwrap_or_else(|_| json!({})),
            etag(&response.headers),
        ))
    }
}
