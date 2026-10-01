#![allow(dead_code)]
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::Value;
use sqlx::{PgPool, postgres::PgConnectOptions};
use tower::ServiceExt;
use warcon_backend::{
    api,
    config::{AppState, Config},
    identity_crypto, migrations,
};
pub struct Db {
    pub state: AppState,
    pub app: Router,
    admin: PgPool,
    name: String,
}
impl Db {
    pub async fn new() -> Self {
        let options: PgConnectOptions =
            std::env::var("TEST_DATABASE_URL").unwrap().parse().unwrap();
        let admin = PgPool::connect_with(options.clone()).await.unwrap();
        let name = format!("warcon_rust_test_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE DATABASE {name}"))
            .execute(&admin)
            .await
            .unwrap();
        let db = PgPool::connect_with(options.database(&name)).await.unwrap();
        migrations::migrate(
            &db,
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../drizzle"),
        )
        .await
        .unwrap();
        let state = AppState {
            db,
            config: Config::for_test(),
        };
        let app = api::router(state.clone());
        Self {
            state,
            app,
            admin,
            name,
        }
    }
    pub async fn user(&self, id: &str, owner: bool) -> String {
        sqlx::query("INSERT INTO \"user\"(id,name,email,username,role,auth_complete) VALUES($1,$1,$2,$1,$3,true)").bind(id).bind(format!("{id}@test.invalid")).bind(if owner{"owner"}else{"member"}).execute(&self.state.db).await.unwrap();
        sqlx::query("INSERT INTO session(id,user_id,token,expires_at) VALUES($1,$1,$2,now()+interval '7 days')").bind(id).bind(format!("{id}-token")).execute(&self.state.db).await.unwrap();
        format!(
            "warcon.session_token={}",
            identity_crypto::signed(&self.state.config.auth_secret, &format!("{id}-token"))
        )
    }
    pub async fn call(
        &self,
        method: &str,
        path: &str,
        body: Value,
        credential: &str,
    ) -> (StatusCode, Value) {
        let mut r = Request::builder()
            .method(method)
            .uri(path)
            .header("origin", "http://localhost:3000")
            .header("content-type", "application/json");
        if credential.starts_with("Bearer ") {
            r = r.header("authorization", credential)
        } else {
            r = r.header("cookie", credential)
        }
        let response = self
            .app
            .clone()
            .oneshot(r.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 8 * 1024 * 1024)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&bytes))),
        )
    }
    pub async fn close(self) {
        drop(self.app);
        self.state.db.close().await;
        sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", self.name))
            .execute(&self.admin)
            .await
            .unwrap();
        self.admin.close().await;
    }
}
