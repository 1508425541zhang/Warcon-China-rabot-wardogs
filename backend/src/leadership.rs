//! Compatible single-writer lease and transaction fencing used by the Bun worker.
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;
pub struct Leadership {
    pub token: String,
    pub label: String,
    pool: PgPool,
}
impl Leadership {
    pub fn new(pool: PgPool, label: String) -> Self {
        Self {
            token: Uuid::new_v4().to_string(),
            label,
            pool,
        }
    }
    pub async fn renew(&self) -> anyhow::Result<bool> {
        let token:Option<String>=sqlx::query_scalar("INSERT INTO worker_ownership(id,token,label,lease_until) VALUES(1,$1,$2,now()+interval '15 seconds') ON CONFLICT(id) DO UPDATE SET token=excluded.token,label=excluded.label,lease_until=excluded.lease_until,acquired_at=CASE WHEN worker_ownership.token=excluded.token THEN worker_ownership.acquired_at ELSE now() END WHERE worker_ownership.token=excluded.token OR worker_ownership.lease_until<now() RETURNING token")
            .bind(&self.token).bind(&self.label).fetch_optional(&self.pool).await?;
        Ok(token.as_deref() == Some(&self.token))
    }
    pub async fn transaction(&self) -> anyhow::Result<Transaction<'_, Postgres>> {
        let mut tx = self.pool.begin().await?;
        let valid: Option<bool> = sqlx::query_scalar(
            "SELECT token=$1 AND lease_until>now() FROM worker_ownership WHERE id=1 FOR SHARE",
        )
        .bind(&self.token)
        .fetch_optional(&mut *tx)
        .await?;
        anyhow::ensure!(valid == Some(true), "Worker lease lost");
        Ok(tx)
    }
    pub async fn valid(&self) -> Result<bool, sqlx::Error> {
        Ok(sqlx::query_scalar::<_, bool>(
            "SELECT token=$1 AND lease_until>now() FROM worker_ownership WHERE id=1",
        )
        .bind(&self.token)
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or(false))
    }
    pub async fn release(&self) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE worker_ownership SET lease_until=to_timestamp(0) WHERE id=1 AND token=$1",
        )
        .bind(&self.token)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
