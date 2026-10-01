//! Offline recovery tool for a machine administrator; never registered as an unauthenticated API.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let username = std::env::args()
        .nth(1)
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    anyhow::ensure!(!username.is_empty(), "Usage: reset-auth <username>");
    let db = sqlx::PgPool::connect_with(warcon_backend::database::options("DATABASE_URL")?).await?;
    let password = warcon_backend::identity_crypto::random_ascii(22);
    let hash = warcon_backend::identity_crypto::hash_password(password.clone()).await?;
    let mut tx = db.begin().await?;
    warcon_backend::api::users::account_lock(&mut tx).await?;
    let id: Option<String> =
        sqlx::query_scalar("SELECT id FROM \"user\" WHERE username=$1 FOR UPDATE")
            .bind(&username)
            .fetch_optional(&mut *tx)
            .await?;
    let id = id.ok_or_else(|| anyhow::anyhow!("Account not found"))?;
    for table in ["two_factor", "passkey", "session"] {
        sqlx::query(&format!("DELETE FROM {table} WHERE user_id=$1"))
            .bind(&id)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("DELETE FROM verification WHERE value=$1 AND (identifier LIKE 'trust-device-%' OR identifier LIKE '2fa-%')").bind(&id).execute(&mut *tx).await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM account WHERE user_id=$1 AND provider_id='credential')",
    )
    .bind(&id)
    .fetch_one(&mut *tx)
    .await?;
    if exists {
        sqlx::query("UPDATE account SET password=$2,updated_at=now() WHERE user_id=$1 AND provider_id='credential'").bind(&id).bind(hash).execute(&mut *tx).await?;
    } else {
        sqlx::query("INSERT INTO account(id,account_id,provider_id,issuer,user_id,password)VALUES($1,$2,'credential','local:credential',$2,$3)").bind(uuid::Uuid::new_v4().to_string()).bind(&id).bind(hash).execute(&mut *tx).await?;
    }
    sqlx::query("UPDATE \"user\" SET two_factor_enabled=false,recovery_key_hash=NULL,recovery_key_at=NULL,auth_complete=false,must_change_password=true,banned=false,ban_reason=NULL,ban_expires=NULL,updated_at=now() WHERE id=$1").bind(&id).execute(&mut *tx).await?;
    tx.commit().await?;
    println!(
        "Sign-in methods reset for @{username}\nTemporary password (change at next sign-in): {password}\nSteam and Discord links retained."
    );
    db.close().await;
    Ok(())
}
