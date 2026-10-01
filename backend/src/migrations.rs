//! Apply the existing Drizzle SQL journal; no schema rewrite or duplicate migration history.
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::{fs, path::Path};
#[derive(Deserialize)]
struct Journal {
    entries: Vec<Entry>,
}
#[derive(Deserialize)]
struct Entry {
    tag: String,
    when: i64,
}
pub async fn migrate(pool: &PgPool, folder: &Path) -> anyhow::Result<usize> {
    let journal: Journal = serde_json::from_slice(&fs::read(folder.join("meta/_journal.json"))?)?;
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('warcon:database-migrations',0))")
        .execute(&mut *tx)
        .await?;
    sqlx::raw_sql("CREATE SCHEMA IF NOT EXISTS drizzle; CREATE TABLE IF NOT EXISTS drizzle.__drizzle_migrations (id SERIAL PRIMARY KEY,hash text NOT NULL,created_at bigint)").execute(&mut *tx).await?;
    let applied =
        sqlx::query("SELECT hash,created_at FROM drizzle.__drizzle_migrations ORDER BY id")
            .fetch_all(&mut *tx)
            .await?;
    anyhow::ensure!(
        applied.len() <= journal.entries.len(),
        "Database is newer than this backend"
    );
    let mut count = 0;
    for (index, entry) in journal.entries.iter().enumerate() {
        anyhow::ensure!(
            entry
                .tag
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "Invalid migration tag"
        );
        let sql = fs::read_to_string(folder.join(format!("{}.sql", entry.tag)))?;
        let hash = hex::encode(Sha256::digest(sql.as_bytes()));
        if let Some(row) = applied.get(index) {
            anyhow::ensure!(
                row.try_get::<i64, _>("created_at")? == entry.when
                    && row.try_get::<String, _>("hash")? == hash,
                "Migration history mismatch at {}",
                entry.tag
            );
            continue;
        }
        sqlx::raw_sql(&sql).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO drizzle.__drizzle_migrations(hash,created_at) VALUES($1,$2)")
            .bind(hash)
            .bind(entry.when)
            .execute(&mut *tx)
            .await?;
        count += 1;
    }
    tx.commit().await?;
    Ok(count)
}
