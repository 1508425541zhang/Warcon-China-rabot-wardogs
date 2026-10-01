use warcon_backend::{
    api,
    config::{AppState, Config},
};
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warcon_backend=info".into()),
        )
        .init();
    let config = Config::from_env()?;
    let listen = config.listen;
    let state = AppState::connect(config).await?;
    let pool = state.db.clone();
    let socket = tokio::net::TcpListener::bind(listen).await?;
    tracing::info!(address=%listen,"Rust backend listening");
    axum::serve(
        socket,
        api::router(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(warcon_backend::shutdown::signal())
    .await?;
    pool.close().await;
    Ok(())
}
