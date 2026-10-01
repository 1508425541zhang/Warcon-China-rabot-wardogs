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
}
impl AppState {
    pub async fn connect(config: Config) -> anyhow::Result<Self> {
        let db = PgPoolOptions::new()
            .max_connections(10)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect(&env::var("DATABASE_URL")?)
            .await?;
        Ok(Self { db, config })
    }
}
