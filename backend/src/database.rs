//! Standard libpq fields avoid interpolating an arbitrary password into a database URL.
use sqlx::postgres::PgConnectOptions;
pub fn options(url_name: &str) -> anyhow::Result<PgConnectOptions> {
    if let Ok(url) = std::env::var(url_name) {
        if !url.trim().is_empty() {
            return Ok(url.parse()?);
        }
    }
    let port = std::env::var("PGPORT")
        .unwrap_or_else(|_| "5432".into())
        .parse()?;
    Ok(PgConnectOptions::new()
        .host(&std::env::var("PGHOST").unwrap_or_else(|_| "localhost".into()))
        .port(port)
        .username(&std::env::var("PGUSER").unwrap_or_else(|_| "warcon".into()))
        .password(&std::env::var("PGPASSWORD").unwrap_or_default())
        .database(&std::env::var("PGDATABASE").unwrap_or_else(|_| "warcon".into())))
}
