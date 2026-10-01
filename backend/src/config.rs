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
        Ok(Self {
            listen,
            origin,
            auth_secret,
            encryption_key,
        })
    }
    pub fn for_test() -> Self {
        Self {
            listen: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4300),
            origin: "http://localhost:3000".into(),
            auth_secret: "test-secret-longer-than-thirty-two-bytes".into(),
            encryption_key: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
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
