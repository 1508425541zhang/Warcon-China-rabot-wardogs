use crate::{
    auth::{self, Actor},
    config::AppState,
    error::{ApiError, Result},
    http::{ApiJson, ApiQuery, integer, string},
};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, HeaderValue, Method},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{Postgres, QueryBuilder};
pub async fn actions(State(app): State<AppState>, headers: HeaderMap) -> Result<Json<Value>> {
    auth::authenticate(&app, &headers, &Method::GET).await?;
    Ok(Json(
        json!({"ok":true,"actions":crate::actions::NAMES.iter().map(|name|{let(cap,mutating)=crate::actions::definition(name).unwrap();(name.to_string(),json!({"cap":cap,"mutating":mutating}))}).collect::<serde_json::Map<_,_>>()}),
    ))
}
async fn cookie_actor(app: &AppState, headers: &HeaderMap, method: &Method) -> Result<Actor> {
    let actor = auth::authenticate(app, headers, method).await?;
    if actor.key.is_some() {
        return Err(ApiError::new(
            axum::http::StatusCode::FORBIDDEN,
            "api_key_forbidden",
            "The org scope is a browser setting.",
        ));
    }
    Ok(actor)
}
pub async fn set_scope(
    State(app): State<AppState>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Response> {
    let actor = cookie_actor(&app, &headers, &Method::PUT).await?;
    let org = string(&body["orgId"], 64);
    if !org.is_empty() {
        let visible:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM organizations o WHERE o.id=$1 AND ($2 OR EXISTS(SELECT 1 FROM org_members m WHERE m.org_id=o.id AND m.user_id=$3)))").bind(&org).bind(actor.owner).bind(&actor.id).fetch_one(&app.db).await?;
        if !visible {
            return Err(ApiError::missing());
        }
    }
    let value = if org.is_empty() { "all" } else { &org };
    let value = percent_encoding::utf8_percent_encode(value, percent_encoding::NON_ALPHANUMERIC);
    let cookie = format!(
        "warcon_scope={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age=31536000{}",
        if app.config.origin.starts_with("https:") {
            "; Secure"
        } else {
            ""
        }
    );
    let mut response =
        Json(json!({"ok":true,"orgId":if org.is_empty(){Value::Null}else{json!(org)}}))
            .into_response();
    response.headers_mut().append(
        "set-cookie",
        HeaderValue::from_str(&cookie).map_err(|_| ApiError::bad("Invalid scope cookie."))?,
    );
    Ok(response)
}
pub async fn clear_scope(State(app): State<AppState>, headers: HeaderMap) -> Result<Response> {
    cookie_actor(&app, &headers, &Method::DELETE).await?;
    Ok((
        [(
            "set-cookie",
            "warcon_scope=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0",
        )],
        Json(json!({"ok":true})),
    )
        .into_response())
}
#[derive(Default)]
pub struct Visibility {
    pub user: String,
    pub servers: Vec<String>,
    pub orgs: Vec<String>,
    pub owner: bool,
}
pub async fn visibility(app: &AppState, actor: &Actor) -> Result<Visibility> {
    let mut v = Visibility {
        user: actor.id.clone(),
        owner: actor.owner && actor.key.is_none(),
        ..Default::default()
    };
    if v.owner {
        return Ok(v);
    }
    if let Some(key) = &actor.key {
        if key.caps.iter().any(|c| c == "audit.read") && key.caps.iter().any(|c| c == "server.view")
        {
            v.servers=sqlx::query_scalar("SELECT s.id FROM servers s JOIN organizations o ON o.id=s.org_id WHERE s.org_id=$1 AND o.suspended_at IS NULL AND ($2::text[] IS NULL OR s.id=ANY($2))").bind(&key.org_id).bind(&key.server_ids).fetch_all(&app.db).await?;
        }
    } else {
        v.orgs=sqlx::query_scalar("SELECT o.id FROM organizations o JOIN org_members m ON m.org_id=o.id WHERE m.user_id=$1 AND m.role='owner' AND o.suspended_at IS NULL").bind(&actor.id).fetch_all(&app.db).await?;
        v.servers=sqlx::query_scalar("SELECT s.id FROM servers s JOIN organizations o ON o.id=s.org_id WHERE o.suspended_at IS NULL AND (s.org_id=ANY($2) OR EXISTS(SELECT 1 FROM server_grants g JOIN org_roles r ON r.id=g.role_id AND r.org_id=s.org_id JOIN org_members m ON m.org_id=s.org_id AND m.user_id=g.user_id WHERE g.server_id=s.id AND g.user_id=$1 AND r.capabilities ? 'audit.read'))").bind(&actor.id).bind(&v.orgs).fetch_all(&app.db).await?;
    }
    Ok(v)
}
#[derive(Default, Deserialize)]
pub struct Filters {
    server: Option<String>,
    actor: Option<String>,
    category: Option<String>,
    action: Option<String>,
    outcome: Option<String>,
    q: Option<String>,
    from: Option<String>,
    to: Option<String>,
    before: Option<String>,
    limit: Option<String>,
    format: Option<String>,
}
fn filter(q: &mut QueryBuilder<'_, Postgres>, v: &Visibility) {
    if !v.owner {
        q.push(" AND (actor_id=")
            .push_bind(v.user.clone())
            .push(" OR org_id=ANY(")
            .push_bind(v.orgs.clone())
            .push(") OR (server_id=ANY(")
            .push_bind(v.servers.clone())
            .push(") AND action NOT IN ('server.create','server.update','server.delete'))) ");
    }
}
fn iso(v: &Value) -> Value {
    v.as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|t| json!(t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)))
        .unwrap_or_else(|| v.clone())
}
fn date(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let s = string(&json!(s), 40);
    chrono::DateTime::parse_from_rfc3339(&s)
        .ok()
        .map(|t| t.to_utc())
        .or_else(|| {
            chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                .ok()
                .and_then(|d| d.and_hms_opt(0, 0, 0))
                .map(|t| t.and_utc())
        })
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S%.f")
                .ok()
                .map(|t| t.and_utc())
        })
}
fn row(r: Value, v: &Visibility) -> Value {
    let names = [
        ("id", "id"),
        ("ts", "ts"),
        ("actorId", "actor_id"),
        ("actorName", "actor_name"),
        ("serverId", "server_id"),
        ("serverName", "server_name"),
        ("orgId", "org_id"),
        ("category", "category"),
        ("action", "action"),
        ("target", "target"),
        ("detail", "detail"),
        ("outcome", "outcome"),
        ("status", "status"),
        ("message", "message"),
        ("userAgent", "user_agent"),
        ("durationMs", "duration_ms"),
    ];
    let mut out = names
        .into_iter()
        .map(|(k, n)| (k.into(), r[n].clone()))
        .collect::<serde_json::Map<String, Value>>();
    out.insert("ts".into(), iso(&r["ts"]));
    if !v.owner
        && r["actor_id"] != v.user
        && !r["org_id"]
            .as_str()
            .is_some_and(|o| v.orgs.iter().any(|id| id == o))
    {
        out.insert("userAgent".into(), json!(""));
    }
    Value::Object(out)
}
pub async fn query(
    app: &AppState,
    v: &Visibility,
    f: &Filters,
    before: Option<i64>,
    limit: usize,
) -> Result<Value> {
    let mut q = QueryBuilder::new("SELECT to_jsonb(a) FROM audit_log a WHERE true");
    filter(&mut q, v);
    for (col, value, max) in [
        ("server_id", &f.server, 64),
        ("actor_id", &f.actor, 64),
        ("category", &f.category, 32),
        ("action", &f.action, 64),
    ] {
        if let Some(value) = value {
            let value = string(&json!(value), max);
            if !value.is_empty() {
                q.push(format!(" AND {col}=")).push_bind(value);
            }
        }
    }
    if let Some(outcome) = f
        .outcome
        .as_deref()
        .filter(|v| ["ok", "error", "denied"].contains(v))
    {
        q.push(" AND outcome=").push_bind(outcome.to_owned());
    }
    for (col, cmp, value) in [("ts", ">=", &f.from), ("ts", "<=", &f.to)] {
        if let Some(at) = value.as_deref().and_then(date) {
            q.push(format!(" AND {col}{cmp}")).push_bind(at.to_utc());
        }
    }
    if let Some(id) = before {
        q.push(" AND id<").push_bind(id);
    }
    if let Some(search) =
        f.q.as_ref()
            .map(|s| string(&json!(s), 200))
            .filter(|s| !s.is_empty())
    {
        let pattern = format!("%{search}%");
        q.push(" AND (");
        for (i, col) in ["actor_name", "server_name", "action", "target", "message"]
            .iter()
            .enumerate()
        {
            if i > 0 {
                q.push(" OR ");
            }
            q.push(format!("{col} LIKE ")).push_bind(pattern.clone());
        }
        q.push(")");
    }
    q.push(" ORDER BY id DESC LIMIT ")
        .push_bind((limit + 1) as i64);
    let mut found: Vec<Value> = q.build_query_scalar().fetch_all(&app.db).await?;
    let more = found.len() > limit;
    found.truncate(limit);
    let next = if more {
        found.last().and_then(|r| r["id"].as_i64())
    } else {
        None
    };
    Ok(json!({"entries":found.into_iter().map(|r|row(r,v)).collect::<Vec<_>>(),"nextBefore":next}))
}
fn before(f: &Filters) -> Option<i64> {
    let n = integer(&json!(f.before), 0, 0, i64::MAX);
    (n > 0).then_some(n)
}
pub async fn audit(
    State(app): State<AppState>,
    headers: HeaderMap,
    ApiQuery(f): ApiQuery<Filters>,
) -> Result<Json<Value>> {
    let actor = auth::authenticate(&app, &headers, &Method::GET).await?;
    let v = visibility(&app, &actor).await?;
    let mut result = query(
        &app,
        &v,
        &f,
        before(&f),
        integer(&json!(f.limit), 100, 1, 500) as usize,
    )
    .await?;
    result["ok"] = json!(true);
    Ok(Json(result))
}
pub async fn meta(State(app): State<AppState>, headers: HeaderMap) -> Result<Json<Value>> {
    let actor = auth::authenticate(&app, &headers, &Method::GET).await?;
    let v = visibility(&app, &actor).await?;
    let mut q = QueryBuilder::new(
        "SELECT DISTINCT ON(action) jsonb_build_object('category',category,'action',action) FROM audit_log WHERE true",
    );
    filter(&mut q, &v);
    q.push(" ORDER BY action,id DESC");
    let mut actions: Vec<Value> = q.build_query_scalar().fetch_all(&app.db).await?;
    actions.sort_by(|a, b| {
        a["category"]
            .as_str()
            .cmp(&b["category"].as_str())
            .then(a["action"].as_str().cmp(&b["action"].as_str()))
    });
    let mut q = QueryBuilder::new(
        "SELECT DISTINCT ON(actor_id) jsonb_build_object('actorId',actor_id,'actorName',actor_name) FROM audit_log WHERE actor_id IS NOT NULL",
    );
    filter(&mut q, &v);
    q.push(" ORDER BY actor_id,id DESC");
    let mut actors: Vec<Value> = q.build_query_scalar().fetch_all(&app.db).await?;
    actors.sort_by_key(|v| v["actorName"].as_str().unwrap_or("").to_lowercase());
    Ok(Json(json!({"ok":true,"actions":actions,"actors":actors})))
}
pub fn csv_cell(v: &Value) -> String {
    let mut text = match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        _ => v.to_string(),
    };
    let formula = v.is_string() && text.starts_with(['=', '+', '-', '@', '\t', '\r']);
    if formula {
        text.insert(0, '\'')
    }
    if formula || text.contains(['"', ',', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}
pub async fn export(
    State(app): State<AppState>,
    headers: HeaderMap,
    ApiQuery(f): ApiQuery<Filters>,
) -> Result<Response> {
    let actor = auth::authenticate(&app, &headers, &Method::GET).await?;
    let v = visibility(&app, &actor).await?;
    let mut all = Vec::new();
    let mut cursor = before(&f);
    while all.len() < 10000 {
        let page = query(&app, &v, &f, cursor, 500.min(10000 - all.len())).await?;
        all.extend(page["entries"].as_array().unwrap().clone());
        cursor = page["nextBefore"].as_i64();
        if cursor.is_none() {
            break;
        }
    }
    let is_json = f.format.as_deref() == Some("json");
    let data = if is_json {
        serde_json::to_string_pretty(&all).unwrap()
    } else {
        let cols = [
            "id",
            "ts",
            "actorName",
            "actorId",
            "serverName",
            "serverId",
            "category",
            "action",
            "target",
            "outcome",
            "status",
            "message",
            "detail",
            "userAgent",
            "durationMs",
        ];
        let mut lines = vec![cols.join(",")];
        for r in all {
            lines.push(
                cols.iter()
                    .map(|k| csv_cell(&r[*k]))
                    .collect::<Vec<_>>()
                    .join(","),
            )
        }
        lines.join("\r\n")
    };
    let stamp = chrono::Utc::now()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        .replace([':', '.'], "-");
    let mut response = data.into_response();
    response.headers_mut().insert(
        "content-type",
        HeaderValue::from_static(if is_json {
            "application/json"
        } else {
            "text/csv; charset=utf-8"
        }),
    );
    response.headers_mut().insert(
        "content-disposition",
        HeaderValue::from_str(&format!(
            "attachment; filename=\"warcon-audit-{stamp}.{}\"",
            if is_json { "json" } else { "csv" }
        ))
        .unwrap(),
    );
    Ok(response)
}
