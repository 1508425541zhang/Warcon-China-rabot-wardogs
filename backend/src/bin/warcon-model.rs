use std::{env, path::Path};
use warcon_backend::{
    model::Predictor,
    model_service::{ModelState, router},
    shutdown,
};
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let predictor = Predictor::load(Path::new(&env::var("MODEL_ARTIFACTS_DIR")?))?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect(&env::var("MODEL_DATABASE_URL")?)
        .await?;
    let host = env::var("MODEL_HOST")
        .unwrap_or_else(|_| "127.0.0.1".into())
        .parse::<std::net::IpAddr>()?;
    let port = env::var("MODEL_PORT")
        .unwrap_or_else(|_| "8091".into())
        .parse::<u16>()?;
    let state = ModelState::new(predictor, pool.clone(), &env::var("MODEL_API_TOKEN")?)?;
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    println!(
        "{}",
        serde_json::json!({"ready":true,"model":state.predictor.manifest.model_id,"runtime":"rust","host":host,"port":listener.local_addr()?.port()})
    );
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown::signal())
        .await?;
    pool.close().await;
    Ok(())
}
