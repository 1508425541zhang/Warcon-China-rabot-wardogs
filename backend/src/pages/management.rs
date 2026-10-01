use super::{Context, go, segment};
use crate::{
    api,
    auth::{Actor, server_scope},
    config::AppState,
    error::{ApiError, Result},
};
use axum::http::StatusCode;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::OnceLock,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

pub async fn remaining(state: &AppState, a: &Actor) -> Result<Value> {
    if a.owner {
        return Ok(Value::Null);
    }
    if !state.config.identity.allow_signup {
        return Ok(json!(0));
    }
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM organizations WHERE created_by=$1")
        .bind(&a.id)
        .fetch_one(&state.db)
        .await?;
    Ok(json!((state.config.organizations.max_per_user - n).max(0)))
}
pub async fn orgs(state: &AppState, a: &Actor) -> Result<Vec<Value>> {
    let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(o)||jsonb_build_object('role',CASE WHEN $1 THEN 'owner' ELSE m.role END) FROM organizations o LEFT JOIN org_members m ON m.org_id=o.id AND m.user_id=$2 WHERE $1 OR m.user_id IS NOT NULL ORDER BY o.name").bind(a.owner).bind(&a.id).fetch_all(&state.db).await?;
    let mut result = vec![];
    for o in rows {
        let kinds = if a.owner {
            vec!["ban".into(), "reserve".into()]
        } else if !o["suspended_at"].is_null() {
            vec![]
        } else {
            api::lists::role_for(state, a, o["id"].as_str().unwrap())
                .await?
                .map(|r| r.kinds)
                .unwrap_or_default()
        };
        result.push(json!({"id":o["id"],"name":o["name"],"slug":o["slug"],"role":o["role"],"suspended":!o["suspended_at"].is_null(),"listKinds":kinds,"allowPublicStatus":o["allow_public_status"],"allowPublicLeaderboards":o["allow_public_leaderboards"]}));
    }
    Ok(result)
}
async fn views(state: &AppState, ids: &[String]) -> Result<Vec<Value>> {
    Ok(sqlx::query_scalar("SELECT jsonb_build_object('id',o.id,'name',o.name,'slug',o.slug,'memberCount',(SELECT count(*) FROM org_members WHERE org_id=o.id),'serverCount',(SELECT count(*) FROM servers WHERE org_id=o.id),'serverLimit',coalesce(o.server_limit,$2::bigint),'customServerLimit',o.server_limit,'suspended',CASE WHEN o.suspended_at IS NULL THEN NULL ELSE jsonb_build_object('at',o.suspended_at,'reason',o.suspended_reason) END,'allowPublicStatus',o.allow_public_status,'allowPublicLeaderboards',o.allow_public_leaderboards,'discordInviteUrl',o.discord_invite_url,'createdBy',CASE WHEN u.id IS NULL THEN NULL ELSE jsonb_build_object('username',coalesce(u.username,''),'name',u.name) END,'createdAt',o.created_at) FROM organizations o LEFT JOIN \"user\" u ON u.id=o.created_by WHERE o.id=ANY($1) ORDER BY o.name").bind(ids).bind(state.config.organizations.max_servers).fetch_all(&state.db).await?)
}
pub async fn app(c: &mut Context) -> Result<Value> {
    let Some(a) = c.account().await? else {
        let cfg = c.get("/api/identity/configuration").await?;
        return Ok(go(
            303,
            if cfg["initialized"] == true {
                "/sign-in"
            } else {
                "/setup"
            },
        ));
    };
    let session = c.get("/api/identity/session").await?;
    if session["gate"]["password"] == true {
        return Ok(go(303, "/account?force=1"));
    }
    if session["gate"]["enrolment"] == true {
        return Ok(go(303, "/account?enrolment=1"));
    }
    let orgs = orgs(&c.state, &a).await?;
    let left = remaining(&c.state, &a).await?;
    let raw = c
        .headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.split(';')
                .find_map(|v| v.trim().strip_prefix("warcon_scope="))
        })
        .map(|v| {
            percent_encoding::percent_decode_str(v)
                .decode_utf8_lossy()
                .into_owned()
        });
    let wanted = raw
        .as_deref()
        .unwrap_or_else(|| session["user"]["defaultOrgId"].as_str().unwrap_or(""));
    let scope = orgs
        .iter()
        .find(|o| o["id"] == wanted)
        .map(|o| json!({"id":o["id"],"name":o["name"],"session":raw.is_some()}));
    let servers =
        api::servers::accessible(&c.state, &a, scope.as_ref().and_then(|s| s["id"].as_str()))
            .await?;
    let account = c.get("/api/identity/account").await?;
    Ok(
        json!({"user":session["user"],"orgs":orgs,"scope":scope,"canManage":a.owner||orgs.iter().any(|o|o["role"]=="owner"&&o["suspended"]==false),"canCreateOrg":left.is_null()||left.as_i64().is_some_and(|v|v>0),"servers":servers,"demoAllowed":crate::mockgame::enabled(),"enrolment":account["status"],"authPolicy":account["policy"],"steamLookup":std::env::var("STEAM_API_KEY").is_ok_and(|v|!v.is_empty())}),
    )
}
pub async fn load(c: &mut Context) -> Result<Value> {
    let source = c.input.source.clone();
    let id = c.param("id").to_owned();
    let encoded = segment(&id);
    match source.as_str() {
        "src/routes/+layout.server.ts" => {
            let session = c.get("/api/identity/session").await?;
            Ok(
                json!({"user":session["user"],"appName":if c.state.config.identity.app_name.is_empty(){"Warcon China"}else{&c.state.config.identity.app_name}}),
            )
        }
        "src/routes/(public)/+layout.server.ts" => Ok(json!({})),
        "src/routes/(app)/+layout.server.ts" => app(c).await,
        "src/routes/(app)/settings/+page.server.ts" => Ok(go(301, "/admin/settings")),
        "src/routes/(app)/users/+page.server.ts" => Ok(go(301, "/admin/users")),
        "src/routes/(app)/admin/+layout.server.ts" => {
            api::users::owner(&c.state, &c.headers, &axum::http::Method::GET).await?;
            Ok(json!({}))
        }
        "src/routes/(app)/admin/+page.server.ts" => {
            Ok(json!({"overview":c.get("/api/admin/overview").await?["overview"]}))
        }
        "src/routes/(app)/admin/settings/+page.server.ts" => {
            api::users::owner(&c.state, &c.headers, &axum::http::Method::GET).await?;
            Ok(json!({"settings":crate::settings::view(&c.state.db).await?}))
        }
        "src/routes/(app)/admin/users/+page.server.ts" => {
            let a = api::users::owner(&c.state, &c.headers, &axum::http::Method::GET).await?;
            let users = c.get("/api/users").await?;
            let servers = api::servers::accessible(&c.state, &a, None).await?;
            let mut roles = serde_json::Map::new();
            for s in &servers {
                let id = s["orgId"].as_str().unwrap();
                if !roles.contains_key(id) {
                    let v = c.get(&format!("/api/orgs/{}/roles", segment(id))).await?;
                    roles.insert(id.into(), v["roles"].clone());
                }
            }
            Ok(json!({"users":users["users"],"servers":servers,"rolesByOrgId":roles}))
        }
        "src/routes/(app)/admin/qq/+page.server.ts" => {
            let a = api::users::owner(&c.state, &c.headers, &axum::http::Method::GET).await?;
            let cfg = c.get("/api/admin/qq").await?;
            let vips = c.get("/api/admin/qq/vips").await?;
            let servers = api::servers::accessible(&c.state, &a, None)
                .await?
                .into_iter()
                .map(|s| json!({"id":s["id"],"name":s["name"],"orgName":s["orgName"]}))
                .collect::<Vec<_>>();
            Ok(
                json!({"config":cfg["config"],"vips":vips["vips"],"servers":servers,"callback":format!("{}/api/qq/webhook",c.state.config.origin)}),
            )
        }
        "src/routes/(app)/qq-link/+page.server.ts" => {
            c.actor().await?;
            c.get("/api/account/qq").await
        }
        "src/routes/(app)/servers/+page.server.ts" => {
            let a = c.actor().await?;
            let context = app(c).await?;
            if context.get("$redirect").is_some() {
                return Ok(context);
            }
            if context["canManage"] != true {
                return Err(ApiError::forbidden());
            }
            let orgs = context["orgs"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|o| o["role"] == "owner" && o["suspended"] == false)
                .cloned()
                .collect::<Vec<_>>();
            let servers = context["servers"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|s| s["manager"] == true)
                .cloned()
                .collect::<Vec<_>>();
            let _ = a;
            Ok(json!({"managed":servers,"ownedOrgs":orgs}))
        }
        "src/routes/(app)/orgs/+page.server.ts" => {
            let a = c.actor().await?;
            let mine = orgs(&c.state, &a).await?;
            let ids = mine
                .iter()
                .filter_map(|o| o["id"].as_str().map(str::to_owned))
                .collect::<Vec<_>>();
            let mut rows = views(&c.state, &ids).await?;
            for row in &mut rows {
                if let Some(o) = mine.iter().find(|o| o["id"] == row["id"]) {
                    row["role"] = o["role"].clone();
                    row["listKinds"] = o["listKinds"].clone();
                }
            }
            Ok(json!({"orgViews":rows}))
        }
        "src/routes/(app)/orgs/[id]/+layout.server.ts" => {
            let a = c.actor().await?;
            let lists = c.get(&format!("/api/orgs/{encoded}/lists")).await?;
            let rows = views(&c.state, std::slice::from_ref(&id)).await?;
            Ok(
                json!({"org":rows.first(),"orgServers":api::servers::accessible(&c.state,&a,Some(&id)).await?,"listsRole":{"owner":lists["role"]=="owner","kinds":lists["kinds"]},"lists":lists}),
            )
        }
        "src/routes/(app)/orgs/[id]/+page.server.ts" => {
            let members = c.get(&format!("/api/orgs/{encoded}/members")).await?;
            let invites = c.get(&format!("/api/orgs/{encoded}/invites")).await?;
            let hooks = c.get(&format!("/api/orgs/{encoded}/webhooks")).await?;
            let roles = c.get(&format!("/api/orgs/{encoded}/roles")).await?;
            let keys = c.get(&format!("/api/orgs/{encoded}/keys")).await?;
            Ok(
                json!({"members":members["members"],"invites":invites["invites"],"webhooks":hooks["webhooks"],"roles":roles["roles"],"keys":keys["keys"],"webhookEvents":webhook_events(),"https":c.state.config.origin.starts_with("https:"),"discord":c.state.config.identity.discord_id.is_some()&&c.state.config.identity.discord_secret.is_some()}),
            )
        }
        "src/routes/(app)/orgs/[id]/roles/+page.server.ts" => {
            Ok(json!({"roles":c.get(&format!("/api/orgs/{encoded}/roles")).await?["roles"]}))
        }
        "src/routes/(app)/orgs/[id]/access/+page.server.ts" => {
            let members = c.get(&format!("/api/orgs/{encoded}/members")).await?;
            let roles = c.get(&format!("/api/orgs/{encoded}/roles")).await?;
            Ok(json!({"members":members["members"],"roles":roles["roles"]}))
        }
        "src/routes/(app)/orgs/[id]/reserved/+page.server.ts"
        | "src/routes/(app)/orgs/[id]/bans/+page.server.ts" => {
            let kind = if source.contains("/reserved/") {
                "reserve"
            } else {
                "ban"
            };
            Ok(
                json!({"entries":c.get(&format!("/api/orgs/{encoded}/lists/{kind}/entries")).await?["entries"]}),
            )
        }
        "src/routes/(app)/orgs/[id]/players/+page.server.ts" => {
            let v = c
                .get(&format!(
                    "/api/orgs/{encoded}/players?{}",
                    c.input.search.clone()
                ))
                .await?;
            Ok(
                json!({"players":v["players"],"total":v["total"],"filters":v["filters"],"pageSize":100}),
            )
        }
        "src/routes/(app)/plugins/+page.server.ts" => {
            let a = c.actor().await?;
            let plugins = c.get("/api/plugins").await?;
            let servers = api::servers::accessible(&c.state, &a, None)
                .await?
                .into_iter()
                .map(|s| json!({"id":s["id"],"name":s["name"],"orgName":s["orgName"]}))
                .collect::<Vec<_>>();
            Ok(
                json!({"plugins":plugins["plugins"],"catalog":super::automation::catalogue(),"pluginServers":servers}),
            )
        }
        "src/routes/(app)/plugins/[pluginId]/+page.server.ts" => {
            let path = format!("/api/plugins/{}", segment(c.param("pluginId")));
            let v = c.get(&path).await?;
            Ok(json!({"plugin":v["plugin"]}))
        }
        "src/routes/(app)/audit/+page.server.ts" => {
            c.actor().await?;
            let v = c
                .get(&format!("/api/audit?{}&limit=100", c.input.search.clone()))
                .await?;
            let meta = c.get("/api/audit/meta").await?;
            let filters = crate::api::activity::filter_view(&c.input.search);
            Ok(
                json!({"entries":v["entries"],"nextBefore":v["nextBefore"],"filters":filters,"actions":meta["actions"],"actors":meta["actors"]}),
            )
        }
        "src/routes/(app)/server/[id]/+layout.server.ts" => server_layout(c, &id).await,
        "src/routes/(app)/server/[id]/slots/+page.server.ts"
        | "src/routes/(app)/server/[id]/bans/+page.server.ts" => {
            let a = c.actor().await?;
            let scope = server_scope(&c.state, &a, &id, "server.view").await?;
            let list_state = api::lists::state_view(&c.state, &a, &scope).await?;
            let org: Value =
                sqlx::query_scalar("SELECT to_jsonb(o) FROM organizations o WHERE id=$1")
                    .bind(&scope.org_id)
                    .fetch_one(&c.state.db)
                    .await?;
            let role = api::lists::role_for(&c.state, &a, &scope.org_id).await?;
            let lists = match role {
                Some(role) => Some(api::lists::view(&c.state, &org, &role).await?),
                None => None,
            };
            Ok(json!({"listState":list_state,"orgLists":lists}))
        }
        "src/routes/(app)/server/[id]/matches/[matchId]/+page.server.ts" => {
            let a = c.actor().await?;
            server_scope(&c.state, &a, &id, "server.view").await?;
            let mid = c
                .param("matchId")
                .parse::<i64>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(ApiError::missing)?;
            let v = api::matches::load_match(&c.state.db, &id, mid)
                .await?
                .ok_or_else(ApiError::missing)?;
            Ok(json!({"match":v}))
        }
        "src/routes/(app)/server/[id]/public/+page.server.ts"
        | "src/routes/(app)/server/[id]/discord/+page.server.ts" => {
            Ok(go(301, &format!("/server/{encoded}/settings")))
        }
        "src/routes/(app)/server/[id]/qq-bindings/+page.server.ts" => {
            let a = c.actor().await?;
            let scope = server_scope(&c.state, &a, &id, "players.moderate").await?;
            let v = c
                .get(&format!(
                    "/api/servers/{encoded}/qq-bindings?{}",
                    c.input.search.clone()
                ))
                .await?;
            let can_manage = scope.caps.iter().any(|v| v == "automation.manage");
            Ok(json!({"bindings":v,"canManage":can_manage}))
        }
        _ if source.starts_with("src/routes/(public)/s/[id]/") => public(c, &source, &id).await,
        _ => Err(ApiError::missing()),
    }
}
pub fn webhook_events() -> Value {
    json!(
        [
            ("bans", "Bans and unbans"),
            (
                "commands",
                "Other game commands (kick, broadcast, map, config…)"
            ),
            ("triggers", "Automation (trigger actions)"),
            ("players", "Player notes and watchlist changes"),
            ("management", "Servers, members, invite links, accounts"),
            ("auth", "Sign-ins and sign-in failures"),
            ("teamkills", "Team kills (from the kill feed)"),
            ("watched", "Watched players joining"),
            ("integrity", "Community Integrity cases and action delivery")
        ]
        .iter()
        .map(|(key, label)| json!({"key":key,"label":label}))
        .collect::<Vec<_>>()
    )
}
type CatalogCache = HashMap<(String, String), (Instant, Value, Value)>;
static CATALOGS: OnceLock<Mutex<CatalogCache>> = OnceLock::new();
pub(super) async fn invalidate_catalog(state: &AppState, id: &str) {
    if let Some(cache) = CATALOGS.get() {
        cache
            .lock()
            .await
            .remove(&(state.config.origin.clone(), id.to_owned()));
    }
}
async fn server_layout(c: &mut Context, id: &str) -> Result<Value> {
    let a = c.actor().await?;
    let scope = server_scope(&c.state, &a, id, "server.view").await?;
    let server = api::servers::accessible(&c.state, &a, Some(&scope.org_id))
        .await?
        .into_iter()
        .find(|s| s["id"] == id)
        .ok_or_else(ApiError::missing)?;
    let cache = CATALOGS.get_or_init(Default::default);
    let key = (c.state.config.origin.clone(), id.to_owned());
    let hit = cache
        .lock()
        .await
        .get(&key)
        .filter(|v| v.0.elapsed() < Duration::from_secs(3600))
        .cloned();
    let mut catalog = json!({"maps":[],"lightings":[],"experiences":[]});
    let mut features = json!({"changeTeam":false,"configDocument":false,"reservedSlots":true,"rotationEdit":true,"rotationSave":true,"liveSettings":true,"serverId":false});
    let mut reachable = true;
    let mut problem = String::new();
    if let Some((_, a, b)) = hit {
        catalog = a;
        features = b;
    } else {
        match c
            .get(&format!("/api/servers/{}/rcon/catalog", segment(id)))
            .await
        {
            Ok(v) => {
                catalog = v["result"].clone();
                if let Ok(v) = c
                    .get(&format!("/api/servers/{}/rcon/capabilities", segment(id)))
                    .await
                {
                    if v["result"]["features"].is_object() {
                        features = v["result"]["features"].clone();
                    }
                }
                let mut cache = cache.lock().await;
                cache.retain(|_, v| v.0.elapsed() < Duration::from_secs(3600));
                if cache.len() < 2000 {
                    cache.insert(key, (Instant::now(), catalog.clone(), features.clone()));
                }
            }
            Err(e) => {
                reachable = false;
                problem = e.message;
            }
        }
    }
    let live: Option<Value> =
        sqlx::query_scalar("SELECT to_jsonb(l) FROM server_live l WHERE server_id=$1")
            .bind(id)
            .fetch_optional(&c.state.db)
            .await?;
    let live = live.unwrap_or(Value::Null);
    Ok(
        json!({"server":server,"catalog":catalog,"features":features,"reachable":reachable,"problem":problem,"identity":{"build":live["build"].as_str().unwrap_or(""),"gameServerId":live["game_server_id"].as_str().unwrap_or(""),"startedAt":live["started_at"]}}),
    )
}
async fn public(c: &mut Context, source: &str, id: &str) -> Result<Value> {
    let feature = if source.ends_with("/[id]/+page.server.ts") || source.contains("/report/") {
        "status"
    } else {
        "leaderboards"
    };
    let ps = api::public::require(&c.state, id, feature).await?;
    let heading = json!({"id":ps["id"],"name":ps["name"],"orgName":ps["orgName"],"features":ps["features"],"discordInviteUrl":ps["discordInviteUrl"]});
    let base = format!("/api/public/servers/{}", segment(id));
    match source {
        "src/routes/(public)/s/[id]/+page.server.ts" => {
            Ok(json!({"view":c.get(&base).await?["server"],"heading":heading}))
        }
        "src/routes/(public)/s/[id]/report/+page.server.ts" => {
            crate::api::public::limit(&c.state, c.peer, &c.headers)?;
            Ok(json!({"heading":heading,"signedIn":c.account().await?.is_some()}))
        }
        "src/routes/(public)/s/[id]/leaderboard/+page.server.ts" => {
            let v = c
                .get(&format!("{base}/leaderboard?{}", c.input.search.clone()))
                .await?;
            let n: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM servers WHERE org_id=$1 AND public_leaderboards",
            )
            .bind(ps["orgId"].as_str().unwrap())
            .fetch_one(&c.state.db)
            .await?;
            Ok(json!({"board":v,"orgScope":n>1,"heading":heading}))
        }
        "src/routes/(public)/s/[id]/matches/+page.server.ts" => Ok(
            json!({"list":c.get(&format!("{base}/matches?{}",c.input.search.clone())).await?,"heading":heading}),
        ),
        "src/routes/(public)/s/[id]/matches/[matchId]/+page.server.ts" => {
            let v = c
                .get(&format!("{base}/matches/{}", segment(c.param("matchId"))))
                .await?;
            let mut m = v.clone();
            if let Some(o) = m.as_object_mut() {
                o.remove("feed");
                o.remove("more");
                o.remove("ok");
            }
            Ok(json!({"match":m,"feed":v["feed"],"more":v["more"],"heading":heading}))
        }
        "src/routes/(public)/s/[id]/players/[steamId]/+page.server.ts" => {
            let mut v = c
                .get(&format!("{base}/players/{}", segment(c.param("steamId"))))
                .await?;
            v["heading"] = heading;
            if let Some(o) = v.as_object_mut() {
                o.remove("ok");
            }
            Ok(v)
        }
        _ => Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "Not found.",
        )),
    }
}
