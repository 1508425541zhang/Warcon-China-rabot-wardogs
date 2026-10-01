use std::{env, path::Path};
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let folder = env::args().nth(1).unwrap_or_else(|| "drizzle".into());
    let url = env::var("DATABASE_URL")?;
    let pool = sqlx::PgPool::connect(&url).await?;
    let count = warcon_backend::migrations::migrate(&pool, Path::new(&folder)).await?;
    println!("{}", serde_json::json!({"ok":true,"applied":count}));
    pool.close().await;
    Ok(())
}
