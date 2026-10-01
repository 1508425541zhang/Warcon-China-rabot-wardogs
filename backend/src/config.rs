use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{
    env,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

#[derive(Clone)]
pub struct Config {
    pub listen: SocketAddr,
    pub origin: String,
    pub auth_secret: String,
    pub encryption_key: String,
    pub identity: IdentityConfig,
    pub organizations: OrganizationConfig,
    pub worker: WorkerConfig,
}
#[derive(Clone, Default)]
pub struct WorkerConfig {
    pub relay_url: Option<String>,
    pub relay_secret: Option<String>,
    pub listen: Option<SocketAddr>,
}
#[derive(Clone)]
pub struct OrganizationConfig {
    pub max_per_user: i64,
    pub max_servers: i64,
}
impl Default for OrganizationConfig {
    fn default() -> Self {
        Self {
            max_per_user: 3,
            max_servers: 10,
        }
    }
}
#[derive(Clone, Default)]
pub struct IdentityConfig {
    pub setup_token: Option<String>,
    pub allow_signup: bool,
    pub app_name: String,
    pub turnstile_key: Option<String>,
    pub turnstile_secret: Option<String>,
    pub discord_id: Option<String>,
    pub discord_secret: Option<String>,
    pub frontend_token: Option<String>,
}
impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let listen = env::var("RUST_BACKEND_BIND")
            .unwrap_or_else(|_| "127.0.0.1:4300".into())
            .parse()?;
        let origin = env::var("ORIGIN")?;
        let auth_secret = env::var("BETTER_AUTH_SECRET")?;
        anyhow::ensure!(
            auth_secret.len() >= 32 && auth_secret != "replace-with-openssl-rand-base64-32",
            "Invalid BETTER_AUTH_SECRET"
        );
        let encryption_key = env::var("ENCRYPTION_KEY")?;
        crate::crypto::decode_key(&encryption_key)?;
        let frontend_token = env::var("RUST_FRONTEND_TOKEN")
            .ok()
            .filter(|s| !s.is_empty());
        anyhow::ensure!(
            frontend_token.as_ref().is_none_or(|s| s.len() >= 32),
            "RUST_FRONTEND_TOKEN must be at least 32 bytes"
        );
        Ok(Self {
            listen,
            origin,
            auth_secret,
            encryption_key,
            worker: WorkerConfig::from_env()?,
            organizations: OrganizationConfig {
                max_per_user: env::var("MAX_ORGS_PER_USER")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .filter(|n| *n > 0)
                    .unwrap_or(3),
                max_servers: env::var("MAX_SERVERS_PER_ORG")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .filter(|n| *n > 0)
                    .unwrap_or(10),
            },
            identity: IdentityConfig {
                setup_token: env::var("SETUP_TOKEN").ok().filter(|s| !s.is_empty()),
                allow_signup: env::var("ALLOW_ORG_SIGNUP").is_ok_and(|s| s == "true" || s == "1"),
                app_name: env::var("APP_NAME").unwrap_or_else(|_| "Warcon".into()),
                turnstile_key: env::var("TURNSTILE_SITE_KEY")
                    .ok()
                    .filter(|s| !s.is_empty()),
                turnstile_secret: env::var("TURNSTILE_SECRET_KEY")
                    .ok()
                    .filter(|s| !s.is_empty()),
                discord_id: env::var("DISCORD_CLIENT_ID").ok().filter(|s| !s.is_empty()),
                discord_secret: env::var("DISCORD_CLIENT_SECRET")
                    .ok()
                    .filter(|s| !s.is_empty()),
                frontend_token,
            },
        })
    }
    pub fn for_test() -> Self {
        Self {
            listen: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4300),
            origin: "http://localhost:3000".into(),
            auth_secret: "test-secret-longer-than-thirty-two-bytes".into(),
            encryption_key: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
            worker: WorkerConfig::default(),
            organizations: OrganizationConfig::default(),
            identity: IdentityConfig {
                app_name: "Warcon".into(),
                ..Default::default()
            },
        }
    }
}
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Config,
    pub runtime: std::sync::Arc<crate::runtime::Runtime>,
}
impl AppState {
    pub async fn connect(config: Config) -> anyhow::Result<Self> {
        let db = PgPoolOptions::new()
            .max_connections(10)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect_with(crate::database::options("DATABASE_URL")?)
            .await?;
        Ok(Self {
            db,
            config,
            runtime: Default::default(),
        })
    }
    pub async fn worker_transaction(
        &self,
    ) -> crate::error::Result<sqlx::Transaction<'_, sqlx::Postgres>> {
        if let Some(leader) = self.runtime.leader.get() {
            leader
                .transaction()
                .await
                .map_err(|_| crate::runtime::lost())
        } else {
            Ok(self.db.begin().await?)
        }
    }
}
impl WorkerConfig {
    fn from_env() -> anyhow::Result<Self> {
        let relay_url = env::var("RELAY_URL").ok().filter(|s| !s.is_empty());
        let relay_secret = env::var("RELAY_SECRET").ok().filter(|s| !s.is_empty());
        let listen = env::var("RUST_WORKER_BIND")
            .ok()
            .map(|s| s.parse())
            .transpose()?;
        if let Some(raw) = &relay_url {
            let url = url::Url::parse(raw)?;
            anyhow::ensure!(
                matches!(url.scheme(), "http" | "https")
                    && url.host_str().is_some()
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.query().is_none()
                    && url.fragment().is_none()
                    && url.path() == "/",
                "RELAY_URL must be a plain HTTP(S) origin"
            );
        }
        if relay_url.is_some() || listen.is_some() {
            anyhow::ensure!(
                relay_secret.as_ref().is_some_and(|s| s.len() >= 32),
                "RELAY_SECRET must be at least 32 bytes"
            );
        }
        Ok(Self {
            relay_url,
            relay_secret,
            listen,
        })
    }
}
