//! Rust worker under the same database lease as the existing worker.
//! Only migrated jobs are registered here; the route/job inventory records remaining work.
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use warcon_backend::{
    leadership::Leadership,
    shutdown,
    steam::{SteamClient, process_next},
};
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("warcon_backend=info")
        .init();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&std::env::var("DATABASE_URL")?)
        .await?;
    let steam = std::env::var("STEAM_API_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(SteamClient::new)
        .transpose()?
        .map(Arc::new);
    let leader = Arc::new(Leadership::new(pool.clone(), "rust-worker".into()));
    anyhow::ensure!(
        leader.renew().await?,
        "Another worker owns this database; Rust worker did not start"
    );
    let stop = CancellationToken::new();
    let signal_stop = stop.clone();
    let signal = tokio::spawn(async move {
        shutdown::signal().await;
        signal_stop.cancel()
    });
    let renew_stop = stop.clone();
    let renew_leader = leader.clone();
    let renew = tokio::spawn(async move {
        let mut clock = tokio::time::interval(std::time::Duration::from_secs(5));
        clock.tick().await;
        loop {
            tokio::select! {
                _=renew_stop.cancelled()=>break,
                _=clock.tick()=>{if !matches!(renew_leader.renew().await,Ok(true)){tracing::error!("Worker ownership lost; stopping");renew_stop.cancel();break}}
            }
        }
    });
    let mut tasks = tokio::task::JoinSet::new();
    if let Some(steam) = steam {
        let profile_stop = stop.clone();
        let profile_leader = leader.clone();
        let profile_steam = steam.clone();
        tasks.spawn(async move {
            let mut clock=tokio::time::interval(std::time::Duration::from_secs(5));clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {tokio::select!{_=profile_stop.cancelled()=>break,_=clock.tick()=>{
                for _ in 0..10 {if profile_stop.is_cancelled(){break}match process_next(&profile_leader,&profile_steam).await {Ok(true)=>{},Ok(false)=>break,Err(_)=>{tracing::error!("Profile refresh pass failed");break}}}
            }}}
        });
        let playtime_stop = stop.clone();
        let playtime_leader = leader.clone();
        tasks.spawn(async move {
            let mut clock=tokio::time::interval(std::time::Duration::from_secs(5));clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {tokio::select!{_=playtime_stop.cancelled()=>break,_=clock.tick()=>{if warcon_backend::playtime::refresh(&playtime_leader,&steam).await.is_err(){tracing::error!("Playtime refresh pass failed");}}}}
        });
    }
    let rollup_stop = stop.clone();
    let rollup_leader = leader.clone();
    tasks.spawn(async move {
        let mut clock=tokio::time::interval(std::time::Duration::from_secs(3600));clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {tokio::select!{_=rollup_stop.cancelled()=>break,_=clock.tick()=>{if warcon_backend::rollups::rollup(&rollup_leader).await.is_err(){tracing::error!("Sample rollup pass failed");}}}}
    });
    tokio::select! { _=stop.cancelled()=>{}, result=tasks.join_next()=>{tracing::error!(ok=?result.map(|r|r.is_ok()),"Background task stopped unexpectedly");stop.cancel();} }
    stop.cancel();
    if tokio::time::timeout(std::time::Duration::from_secs(20), async {
        while tasks.join_next().await.is_some() {}
    })
    .await
    .is_err()
    {
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
    }
    renew.await?;
    signal.abort();
    leader.release().await?;
    pool.close().await;
    Ok(())
}
