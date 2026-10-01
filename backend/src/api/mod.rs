pub mod activity;
pub mod analytics;
pub mod automation;
pub mod community;
pub mod feed_setup;
pub mod ingest;
pub mod integrity_cases;
pub mod integrity_reports;
pub mod integrity_settings;
pub mod keys;
pub mod kills;
pub mod lists;
pub mod live;
pub mod matches;
pub mod model_settings;
pub mod notes;
pub mod orgs;
pub mod outbox;
pub mod plugins;
pub mod public;
pub mod qq;
pub mod qq_bindings;
pub mod rcon_actions;
pub mod roles;
pub mod servers;
pub mod settings;
pub mod users;
pub mod webhooks;
use crate::config::AppState;
use axum::{
    Json, Router,
    extract::Request,
    http::{HeaderValue, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{delete, get, post},
};
use serde_json::{Value, json};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/actions",get(activity::actions))
        .route("/api/scope",axum::routing::put(activity::set_scope).delete(activity::clear_scope))
        .route("/api/audit",get(activity::audit))
        .route("/api/audit/meta",get(activity::meta))
        .route("/api/audit/export",get(activity::export))
        .route("/api/identity/configuration",get(crate::identity::configuration))
        .route("/api/identity/session",get(crate::identity::session))
        .route("/api/identity/setup",post(crate::identity::setup))
        .route("/api/identity/register",post(crate::identity_signup::register))
        .route("/api/identity/login",post(crate::identity::login))
        .route("/api/identity/verify",post(crate::identity::verify))
        .route("/api/identity/recover",post(crate::identity::recover))
        .route("/api/identity/logout",post(crate::identity::logout))
        .route("/api/identity/account",get(crate::identity::account))
        .route("/api/identity/account/{action}",post(crate::identity::account_action))
        .route("/api/identity/providers/{provider}",post(crate::oauth::begin))
        .route("/auth/steam/callback",get(crate::oauth::steam_callback))
        .route("/api/auth/callback/discord",get(crate::oauth::discord_callback))
        .route("/api/passkeys",get(crate::passkeys::list).post(crate::passkeys::register))
        .route("/api/passkeys/auth-options",post(crate::passkeys::auth_options))
        .route("/api/passkeys/register-options",post(crate::passkeys::register_options))
        .route("/api/passkeys/auth",post(crate::passkeys::authenticate))
        .route("/api/passkeys/{id}",delete(crate::passkeys::delete))
        .route("/api/health",get(health))
        .route("/api/admin/qq",get(qq::get).put(qq::put).post(qq::test))
        .route("/api/qq/webhook",post(qq::webhook))
        .route("/api/admin/qq/vips",get(qq::get_vips).put(qq::put_vips))
        .route("/api/servers/{id}/qq-bindings",get(qq_bindings::get).post(qq_bindings::post))
        .route("/api/servers/{id}/community",get(community::get).post(community::post))
        .route("/api/account/qq",get(qq_bindings::account))
        .route("/api/account/qq/bind",post(qq_bindings::bind))
        .route("/api/account/qq/unbind",post(qq_bindings::unbind))
        .route("/api/orgs/{id}/webhooks",get(webhooks::list).post(webhooks::create))
        .route("/api/orgs/{id}/webhooks/{webhook_id}",axum::routing::patch(webhooks::update).delete(webhooks::delete))
        .route("/api/orgs/{id}/webhooks/{webhook_id}/test",post(webhooks::test))
        .route("/api/orgs/{id}/webhooks/{webhook_id}/card",post(webhooks::card))
        .route("/api/orgs/{id}/integrity/rules",get(integrity_settings::get_rules).put(integrity_settings::put_rules))
        .route("/api/orgs/{id}/integrity/model",get(model_settings::get).put(model_settings::put).post(model_settings::test))
        .route("/api/orgs/{id}/integrity/cases/{case_id}/labels",post(integrity_cases::label))
        .route("/api/integrity/reports",post(integrity_reports::post))
        .route("/api/reports",post(integrity_reports::post))
        .route("/api/orgs/{id}/integrity/mode",axum::routing::put(integrity_settings::put_mode))
        .route("/api/orgs/{id}/integrity/enforcement",get(integrity_settings::get_enforcement).put(integrity_settings::put_enforcement))
        .route("/api/orgs/{id}/integrity/weapons",get(integrity_settings::get_weapons).put(integrity_settings::put_weapon).delete(integrity_settings::delete_weapon))
        .route("/api/live",get(live::get))
        .route("/api/live/events",get(live::events))
        .route("/api/servers",get(servers::list).post(servers::create))
        .route("/api/servers/{id}",axum::routing::patch(servers::update).delete(servers::delete))
        .route("/api/servers/{id}/grants",get(servers::grants).put(servers::set_grants))
        .route("/api/servers/{id}/test",post(servers::test))
        .route("/api/servers/{id}/rcon/{action}",get(rcon_actions::get).post(rcon_actions::post))
        .route("/api/orgs/{id}/lists",get(lists::org_view))
        .route("/api/orgs/{id}/lists/{kind}/entries",get(lists::entries).post(lists::add))
        .route("/api/orgs/{id}/lists/{kind}/entries/{steam_id}",axum::routing::patch(lists::update).delete(lists::remove))
        .route("/api/orgs/{id}/lists/sync",post(lists::sync))
        .route("/api/orgs/{id}/lists/import",get(lists::import_get).post(lists::import_post))
        .route("/api/servers/{id}/lists/{kind}/entries",post(lists::server_add))
        .route("/api/servers/{id}/lists/{kind}/entries/{steam_id}",axum::routing::patch(lists::server_update).delete(lists::server_remove))
        .route("/api/servers/{id}/lists/state",get(lists::server_state))
        .route("/api/servers/{id}/lists/sync",post(lists::server_sync))
        .route("/api/users",get(users::list).post(users::create))
        .route("/api/users/{id}",axum::routing::patch(users::update).delete(users::delete))
        .route("/api/users/{id}/grants",axum::routing::put(users::grants))
        .route("/api/orgs",get(orgs::list).post(orgs::create))
        .route("/api/orgs/{id}",axum::routing::patch(orgs::update).delete(orgs::delete))
        .route("/api/orgs/{id}/members",get(orgs::members))
        .route("/api/orgs/{id}/members/{user_id}",axum::routing::patch(orgs::member_role).delete(orgs::remove_member))
        .route("/api/orgs/{id}/members/{user_id}/grants",axum::routing::put(orgs::member_grants))
        .route("/api/orgs/{id}/invites",get(orgs::invites).post(orgs::create_invite))
        .route("/api/orgs/{id}/invites/{invite_id}",delete(orgs::revoke_invite))
        .route("/api/identity/invites/{token}",get(orgs::invite_view).post(orgs::join))
        .route("/api/settings",get(settings::get).put(settings::put))
        .route("/api/plugins",get(plugins::list).post(plugins::create))
        .route("/api/plugins/{id}",get(plugins::get).put(plugins::update).delete(plugins::delete))
        .route("/api/plugins/{id}/data",get(plugins::data))
        .route("/api/servers/{id}/leaderboard",get(public::private_board))
        .route("/api/public/servers/{id}",get(public::status))
        .route("/api/public/servers/{id}/leaderboard",get(public::board))
        .route("/api/public/servers/{id}/players/{steam_id}",get(public::player))
        .route("/api/public/servers/{id}/matches",get(public::matches))
        .route("/api/public/servers/{id}/matches/{match_id}",get(public::match_detail))
        .route("/api/ingest/events",post(ingest::post))
        .route("/api/orgs/{id}/roles",get(roles::list).post(roles::create))
        .route("/api/orgs/{id}/keys",get(keys::list).post(keys::create))
        .route("/api/orgs/{id}/keys/{key_id}",delete(keys::revoke))
        .route("/api/servers/{id}/outbox",get(outbox::list))
        .route("/api/servers/{id}/analytics",get(analytics::analytics))
        .route("/api/servers/{id}/cash",get(analytics::cash))
        .route("/api/servers/{id}/feed",get(feed_setup::get).post(feed_setup::post).delete(feed_setup::delete))
        .route("/api/servers/{id}/numeric-limits",post(automation::numeric))
        .route("/api/servers/{id}/faction-lock",post(automation::faction_lock))
        .route("/api/servers/{id}/skill-balance",post(automation::skill_balance))
        .route("/api/servers/{id}/weapon-restrictions",post(automation::weapons))
        .route("/api/orgs/{id}/roles/{role_id}",axum::routing::patch(roles::update).delete(roles::delete))
        .route("/api/orgs/{id}/roles/{role_id}/reset",post(roles::reset))
        .route("/api/servers/{id}/kills",get(kills::list))
        .route("/api/servers/{id}/matches",get(matches::list))
        .route("/api/servers/{id}/matches/{match_id}",get(matches::detail))
        .route("/api/servers/{id}/players/{steam_id}/notes",post(notes::add))
        .route("/api/servers/{id}/players/{steam_id}/watch",axum::routing::put(notes::watch))
        .route("/api/servers/{id}/players/{steam_id}/notes/{note_id}",delete(notes::delete))
        .fallback(||async{(StatusCode::NOT_FOUND,Json(json!({"ok":false,"error":{"code":"not_found","message":"Route is not implemented by the Rust backend."}})))})
        .layer(middleware::from_fn(response_headers))
        .with_state(state)
}
async fn response_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    if !response.headers().contains_key("cache-control") {
        response
            .headers_mut()
            .insert("cache-control", HeaderValue::from_static("no-store"));
    }
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
}
async fn health() -> Json<Value> {
    Json(json!({"ok":true,"service":"warcon"}))
}
