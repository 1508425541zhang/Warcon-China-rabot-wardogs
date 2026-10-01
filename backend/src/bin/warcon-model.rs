use std::{env, path::Path};
use warcon_backend::{
    model::Predictor,
    model_service::{ModelState, router},
    shutdown,
};
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let root = env::var("MODEL_ARTIFACTS_DIR")?;
    let metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(Path::new(&root).join("manifest.json"))?)?;
    let pool = if metadata["channels"].as_u64().unwrap_or(27) > 27 {
        Some(
            sqlx::postgres::PgPoolOptions::new()
                .max_connections(2)
                .acquire_timeout(std::time::Duration::from_secs(10))
                .connect_with(warcon_backend::database::options("MODEL_DATABASE_URL")?)
                .await?,
        )
    } else {
        None
    };
    let host = env::var("MODEL_HOST")
        .unwrap_or_else(|_| "127.0.0.1".into())
        .parse::<std::net::IpAddr>()?;
    let port = env::var("MODEL_PORT")
        .unwrap_or_else(|_| "8091".into())
        .parse::<u16>()?;
    let state = if let Some(pool) = &pool {
        ModelState::new(
            Predictor::load(Path::new(&root))?,
            pool.clone(),
            &env::var("MODEL_API_TOKEN")?,
        )?
    } else {
        ModelState::legacy(
            warcon_backend::legacy_model::Predictor::load(Path::new(&root))?,
            &env::var("MODEL_API_TOKEN")?,
        )?
    };
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    println!(
        "{}",
        serde_json::json!({"ready":true,"model":state.predictor.manifest()["model_id"],"runtime":"rust","host":host,"port":listener.local_addr()?.port()})
    );
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown::signal())
        .await?;
    if let Some(pool) = pool {
        pool.close().await;
    }
    Ok(())
}
