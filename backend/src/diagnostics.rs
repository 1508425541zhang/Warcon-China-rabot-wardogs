//! In-process counters and owner-only fleet diagnostics. No per-server request metrics labels.
use crate::{
    config::AppState,
    error::{ApiError, Result},
};
use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use chrono::Utc;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::Instant,
};
use subtle::ConstantTimeEq;
#[derive(Default)]
struct Counters {
    requests: u64,
    public: u64,
    errors: u64,
    seconds: f64,
    ok: u64,
    failed: u64,
    observation_seconds: f64,
    rate: u64,
    feed_rate: u64,
    feed: [u64; 6],
    http: HashMap<(String, String, u16), u64>,
    delivery: [u64; 4],
    delivery_active: u64,
    delivery_at: i64,
    observation_buckets: [u64; 9],
    http_seconds: HashMap<String, (u64, f64, [u64; 10])>,
}
const OBSERVATION_BUCKETS: [f64; 9] = [0.05, 0.1, 0.25, 0.5, 1., 2.5, 5., 10., 30.];
const HTTP_BUCKETS: [f64; 10] = [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1., 2.5, 5.];
fn counters() -> &'static Mutex<Counters> {
    static C: OnceLock<Mutex<Counters>> = OnceLock::new();
    C.get_or_init(Default::default)
}
pub fn observation(ok: bool, seconds: f64) {
    let mut c = counters().lock().unwrap_or_else(|e| e.into_inner());
    if ok {
        c.ok += 1
    } else {
        c.failed += 1
    }
    c.observation_seconds += seconds;
    for (count, bound) in c.observation_buckets.iter_mut().zip(OBSERVATION_BUCKETS) {
        *count += u64::from(seconds <= bound);
    }
}
pub fn limited(feed: bool) {
    let mut c = counters().lock().unwrap_or_else(|e| e.into_inner());
    c.rate += 1;
    if feed {
        c.feed_rate += 1
    }
}
pub fn feed(accepted: u64, skipped: u64, duplicates: u64) {
    let mut c = counters().lock().unwrap_or_else(|e| e.into_inner());
    c.feed[3] += accepted;
    c.feed[4] += skipped;
    c.feed[5] += duplicates;
}
pub fn delivery(outcome: &str, count: u64) {
    let index = match outcome {
        "delivered" => 0,
        "failed" => 1,
        "skipped" => 2,
        "unknown" => 3,
        _ => return,
    };
    counters()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .delivery[index] += count;
}
pub fn delivery_pass() {
    counters()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .delivery_at = Utc::now().timestamp_millis();
}
pub struct DeliveryGuard;
pub fn delivery_active() -> DeliveryGuard {
    counters()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .delivery_active += 1;
    DeliveryGuard
}
impl Drop for DeliveryGuard {
    fn drop(&mut self) {
        let mut c = counters().lock().unwrap_or_else(|e| e.into_inner());
        c.delivery_active = c.delivery_active.saturating_sub(1);
    }
}
pub fn process() -> Value {
    let c = counters().lock().unwrap_or_else(|e| e.into_inner());
    let rss = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines().find_map(|l| {
                l.strip_prefix("VmRSS:")
                    .and_then(|s| s.split_whitespace().next())
                    .and_then(|s| s.parse::<u64>().ok())
            })
        })
        .map(|n| n * 1024);
    json!({"at":Utc::now().timestamp_millis(),"build":{"version":env!("CARGO_PKG_VERSION"),"commit":std::env::var("WARCON_COMMIT").ok().filter(|s|!s.is_empty()).unwrap_or_else(||option_env!("WARCON_COMMIT").unwrap_or("unknown").into())},"observations":{"ok":c.ok,"failed":c.failed,"seconds":c.observation_seconds},"requests":{"total":c.requests,"public":c.public,"errors":c.errors,"seconds":c.seconds},"feed":{"posts":c.feed[0],"unauthorized":c.feed[1],"rejected":c.feed[2],"kills":c.feed[3],"skipped":c.feed[4],"duplicates":c.feed[5]},"rateLimited":{"total":c.rate,"feed":c.feed_rate},"rssBytes":rss,"eventLoopLagP99":null})
}
pub async fn record(r: Request, next: Next) -> Response {
    let now = Instant::now();
    let path = r.uri().path().to_owned();
    let route = r
        .extensions()
        .get::<axum::extract::MatchedPath>()
        .map(|r| r.as_str().to_owned())
        .unwrap_or_else(|| "unmatched".into());
    let method = r.method().to_string();
    let res = next.run(r).await;
    let status = res.status().as_u16();
    let seconds = now.elapsed().as_secs_f64();
    let mut c = counters().lock().unwrap_or_else(|e| e.into_inner());
    c.requests += 1;
    c.public += u64::from(path.starts_with("/api/public/"));
    c.errors += u64::from(status >= 500);
    c.seconds += seconds;
    if c.http_seconds.len() < 2000 || c.http_seconds.contains_key(&route) {
        let histogram = c.http_seconds.entry(route.clone()).or_default();
        histogram.0 += 1;
        histogram.1 += seconds;
        for (count, bound) in histogram.2.iter_mut().zip(HTTP_BUCKETS) {
            *count += u64::from(seconds <= bound);
        }
    }
    if c.http.len() < 2000
        || c.http
            .contains_key(&(route.clone(), method.clone(), status))
    {
        *c.http.entry((route, method, status)).or_default() += 1;
    }
    if path == "/api/ingest/events" {
        c.feed[if status < 400 {
            0
        } else if status == 401 {
            1
        } else {
            2
        }] += 1
    }
    res
}
pub async fn worker(state: &AppState) -> Result<Value> {
    if crate::gateway::remote(state) {
        return crate::gateway::call(state, "health", json!({}))
            .await
            .map_err(|_| {
                ApiError::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "worker_unavailable",
                    "Worker relay unavailable.",
                )
            });
    }
    stats(state).await
}
pub async fn stats(state: &AppState) -> Result<Value> {
    let mut v = state
        .runtime
        .poller
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let set = crate::settings::load(&state.db).await?;
    let owner = if let Some(l) = state.runtime.leader.get() {
        l.valid().await?
    } else {
        false
    };
    let depth:Value=sqlx::query_scalar("SELECT jsonb_build_object('pending',count(*),'oldestMs',extract(epoch FROM(now()-min(created_at)))*1000)FROM outbox WHERE state='pending'").fetch_one(&state.db).await?;
    if !v.is_object() {
        v = json!({"enabled":false,"servers":0,"players":0,"active":0,"behind":0,"stuck":0,"launched":0,"beatAgoMs":null,"tiers":{"watched":0,"hot":0,"idle":0,"offline":0}})
    }
    let beat = v.get("beatAt").and_then(Value::as_i64);
    v["beatAgoMs"] = json!(beat.map(|t| (Utc::now().timestamp_millis() - t).max(0)));
    v.as_object_mut().unwrap().remove("beatAt");
    v["owner"] = json!(owner);
    v["lanes"] = crate::dispatcher::stats();
    v["delivery"] = depth;
    {
        let c = counters().lock().unwrap_or_else(|e| e.into_inner());
        for (i, key) in ["delivered", "failed", "skipped", "unknown"]
            .iter()
            .enumerate()
        {
            v["delivery"][*key] = json!(c.delivery[i]);
        }
        v["delivery"]["inFlight"] = json!(c.delivery_active);
        v["delivery"]["lastPassAt"] = json!(c.delivery_at);
    }
    v["concurrency"] = set["concurrency"].clone();
    v["settingsVersion"] = json!(state.runtime.track_settings(&set));
    let lease:Option<Value> = sqlx::query_scalar("SELECT jsonb_build_object('token',left(token,8),'since',extract(epoch FROM acquired_at)*1000,'lastRenewAt',extract(epoch FROM lease_until-interval '15 seconds')*1000) FROM worker_ownership WHERE id=1").fetch_optional(&state.db).await?;
    v["ownership"] = lease.unwrap_or(json!({"token":"","since":0,"lastRenewAt":0}));
    v["ownership"]["owner"] = json!(owner);
    v["ownership"]["stopping"] = json!(state.runtime.stop.is_cancelled());
    v["process"] = process();
    v["cadence"] = json!({"watched":{"players":set["watchedPlayersMs"],"status":set["watchedStatusMs"]},"hot":{"players":set["hotPlayersMs"],"status":set["hotStatusMs"]},"idle":{"players":set["idleMs"],"status":set["idleMs"]}});
    Ok(v)
}
fn token(h: &HeaderMap) -> bool {
    std::env::var("METRICS_TOKEN")
        .ok()
        .filter(|s| !s.is_empty())
        .is_some_and(|secret| {
            h.get("authorization")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .as_bytes()
                .ct_eq(format!("Bearer {secret}").as_bytes())
                .into()
        })
}
pub async fn health(State(state): State<AppState>, h: HeaderMap) -> Json<Value> {
    let owner = crate::auth::authenticate(&state, &h, &Method::GET)
        .await
        .is_ok_and(|a| a.owner && a.key.is_none());
    if !owner && !token(&h) {
        return Json(json!({"ok":true,"service":"warcon"}));
    }
    let worker = worker(&state)
        .await
        .unwrap_or(json!({"enabled":false,"error":"Worker unavailable."}));
    Json(json!({"ok":true,"service":"warcon","role":"web","worker":worker}))
}
pub async fn overview(
    State(state): State<AppState>,
    h: HeaderMap,
    crate::http::ApiQuery(q): crate::http::ApiQuery<HashMap<String, String>>,
) -> Result<Json<Value>> {
    let a = crate::auth::authenticate(&state, &h, &Method::GET).await?;
    if !a.owner || a.key.is_some() {
        return Err(ApiError::forbidden());
    }
    let fleet:Value=sqlx::query_scalar("SELECT jsonb_build_object('orgs',(SELECT count(*)FROM organizations),'orgsWeek',(SELECT count(*)FROM organizations WHERE created_at>now()-interval '7 days'),'users',(SELECT count(*)FROM \"user\"),'usersWeek',(SELECT count(DISTINCT user_id)FROM session WHERE updated_at>now()-interval '7 days'),'servers',(SELECT count(*)FROM servers),'serversPublic',(SELECT count(*)FROM servers WHERE public_status OR public_leaderboards),'serversOk',(SELECT count(*)FROM server_live WHERE ok),'serversObserved',(SELECT count(*)FROM server_live WHERE observed_at IS NOT NULL),'serversFeeding',(SELECT count(*)FROM server_live WHERE feed_at>now()-interval '2 minutes'),'players',(SELECT coalesce(sum(player_count),0)FROM server_live WHERE ok))").fetch_one(&state.db).await?;
    let builds:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('build',build,'count',count(*))FROM server_live GROUP BY build ORDER BY count(*)DESC,build").fetch_all(&state.db).await?;
    let bytes: f64 = sqlx::query_scalar("SELECT pg_database_size(current_database())::float8")
        .fetch_one(&state.db)
        .await?;
    let mut tables:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('name',c.relname,'rows',greatest(c.reltuples,0)::float8,'bytes',pg_total_relation_size(c.oid)::float8)FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND c.relkind IN('r','p')").fetch_all(&state.db).await?;
    let timescale: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_extension WHERE extname='timescaledb')")
            .fetch_one(&state.db)
            .await?;
    if timescale {
        let hyper:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('name',hypertable_name,'bytes',hypertable_size(format('%I.%I',hypertable_schema,hypertable_name))::float8,'rows',approximate_row_count(format('%I.%I',hypertable_schema,hypertable_name))::float8)FROM timescaledb_information.hypertables WHERE hypertable_schema='public'").fetch_all(&state.db).await?;
        for t in hyper {
            tables.retain(|r| r["name"] != t["name"]);
            tables.push(t)
        }
    }
    tables.sort_by(|a, b| {
        b["bytes"]
            .as_f64()
            .partial_cmp(&a["bytes"].as_f64())
            .unwrap()
    });
    type Seen = HashMap<String, (Instant, Value)>;
    static CACHE: OnceLock<tokio::sync::Mutex<Seen>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().await;
    let db: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&state.db)
        .await?;
    let seen = if q.get("recount").is_none_or(|s| s != "1")
        && cache
            .get(&db)
            .is_some_and(|r| r.0.elapsed().as_secs() < 300)
    {
        cache[&db].1.clone()
    } else {
        let v:Value=sqlx::query_scalar("SELECT jsonb_build_object('today',count(DISTINCT steam_id)FILTER(WHERE last_seen>now()-interval '1 day'),'month',count(DISTINCT steam_id)FILTER(WHERE last_seen>now()-interval '30 days'),'all',count(DISTINCT steam_id),'at',now())FROM player_sessions").fetch_one(&state.db).await?;
        cache.retain(|_, r| r.0.elapsed().as_secs() < 300);
        cache.insert(db, (Instant::now(), v.clone()));
        v
    };
    drop(cache);
    Ok(Json(
        json!({"ok":true,"overview":{"at":Utc::now(),"worker":worker(&state).await.ok(),"web":process(),"metricsOn":std::env::var("METRICS_TOKEN").is_ok_and(|s|!s.is_empty()),"fleet":fleet,"builds":builds,"database":{"bytes":bytes,"tables":tables},"seen":seen}}),
    ))
}
pub async fn metrics(State(state): State<AppState>, h: HeaderMap) -> Result<Response> {
    if std::env::var("METRICS_TOKEN").is_err()
        || std::env::var("METRICS_TOKEN").is_ok_and(|s| s.is_empty())
    {
        return Err(ApiError::missing());
    }
    if !token(&h) {
        return Err(ApiError::unauthorized());
    }
    let process = process();
    let stats = stats(&state).await?;
    let mut body = String::new();
    fn escape(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    }
    fn histogram(
        body: &mut String,
        name: &str,
        labels: &str,
        bounds: &[f64],
        buckets: &[u64],
        count: u64,
        sum: f64,
    ) {
        for (bound, value) in bounds.iter().zip(buckets) {
            body.push_str(&format!(
                "{name}_bucket{{{labels}le=\"{bound}\"}} {value}\n"
            ));
        }
        body.push_str(&format!("{name}_bucket{{{labels}le=\"+Inf\"}} {count}\n"));
        let labels = labels.trim_end_matches(',');
        let labels = if labels.is_empty() {
            String::new()
        } else {
            format!("{{{labels}}}")
        };
        body.push_str(&format!(
            "{name}_sum{labels} {sum}\n{name}_count{labels} {count}\n"
        ));
    }
    {
        let c = counters().lock().unwrap_or_else(|e| e.into_inner());
        body.push_str(&format!(
            "warcon_build_info{{version=\"{}\",commit=\"{}\"}} 1\n",
            escape(process["build"]["version"].as_str().unwrap_or("")),
            escape(process["build"]["commit"].as_str().unwrap_or(""))
        ));
        histogram(
            &mut body,
            "warcon_observation_seconds",
            "",
            &OBSERVATION_BUCKETS,
            &c.observation_buckets,
            c.ok + c.failed,
            c.observation_seconds,
        );
        for (route, (count, sum, buckets)) in &c.http_seconds {
            histogram(
                &mut body,
                "warcon_http_request_seconds",
                &format!("route=\"{}\",", escape(route)),
                &HTTP_BUCKETS,
                buckets,
                *count,
                *sum,
            );
        }
        for (i, outcome) in ["delivered", "failed", "skipped", "unknown"]
            .iter()
            .enumerate()
        {
            body.push_str(&format!(
                "warcon_deliveries_total{{outcome=\"{outcome}\"}} {}\n",
                c.delivery[i]
            ));
        }
        for (i, outcome) in ["accepted", "unauthorized", "rejected"].iter().enumerate() {
            body.push_str(&format!(
                "warcon_feed_posts_total{{outcome=\"{outcome}\"}} {}\n",
                c.feed[i]
            ));
        }
        for (i, result) in ["accepted", "skipped", "duplicate"].iter().enumerate() {
            body.push_str(&format!(
                "warcon_feed_kills_total{{result=\"{result}\"}} {}\n",
                c.feed[3 + i]
            ));
        }
        body.push_str(&format!("warcon_rate_limited_total{{scope=\"feed\"}} {}\nwarcon_rate_limited_total{{scope=\"other\"}} {}\n",c.feed_rate,c.rate.saturating_sub(c.feed_rate)));
        for ((r, m, s), n) in &c.http {
            let escape = |s: &str| {
                s.replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\n', "\\n")
            };
            body.push_str(&format!(
                "warcon_http_requests_total{{route=\"{}\",method=\"{}\",status=\"{}\"}} {}\n",
                escape(r),
                escape(m),
                s,
                n
            ));
        }
    }
    for tier in ["watched", "hot", "idle", "offline"] {
        body.push_str(&format!(
            "warcon_servers{{tier=\"{tier}\"}} {}\n",
            stats["tiers"][tier].as_u64().unwrap_or(0)
        ));
    }
    if let Some(ms) = stats["delivery"]["oldestMs"].as_f64() {
        body.push_str(&format!("warcon_outbox_oldest_seconds {}\n", ms / 1000.));
    }
    let fleet:Vec<(String,i64)>=sqlx::query_as("SELECT 'organizations',count(*)FROM organizations UNION ALL SELECT 'users',count(*)FROM \"user\" UNION ALL SELECT 'servers',count(*)FROM servers UNION ALL SELECT 'org_members',count(*)FROM org_members UNION ALL SELECT 'webhooks',count(*)FROM webhooks UNION ALL SELECT 'triggers',count(*)FROM triggers").fetch_all(&state.db).await?;
    for (table, count) in fleet {
        body.push_str(&format!("warcon_fleet{{table=\"{table}\"}} {count}\n"));
    }
    for (name, v) in [
        ("process_resident_memory_bytes", process["rssBytes"].clone()),
        (
            "warcon_observations_total{outcome=\"ok\"}",
            process["observations"]["ok"].clone(),
        ),
        (
            "warcon_observations_total{outcome=\"failed\"}",
            process["observations"]["failed"].clone(),
        ),
        ("warcon_players_online", stats["players"].clone()),
        ("warcon_observations_in_flight", stats["active"].clone()),
        (
            "warcon_observation_concurrency",
            stats["concurrency"].clone(),
        ),
        ("warcon_servers_behind", stats["behind"].clone()),
        ("warcon_observations_stuck", stats["stuck"].clone()),
        ("warcon_lanes_busy", stats["lanes"]["busy"].clone()),
        ("warcon_lanes_queued", stats["lanes"]["queued"].clone()),
        (
            "warcon_worker_lease_held",
            json!(u8::from(stats["owner"] == true)),
        ),
        (
            "warcon_outbox_pending",
            stats["delivery"]["pending"].clone(),
        ),
    ] {
        if let Some(n) = v.as_f64() {
            body.push_str(&format!("{name} {n}\n"))
        }
    }
    let queues:Value=sqlx::query_scalar("SELECT jsonb_build_object('pending',count(*)FILTER(WHERE state='pending'),'processing',count(*)FILTER(WHERE state='processing'),'retrying',count(*)FILTER(WHERE state IN('pending','processing')AND attempts>0),'oldestSeconds',extract(epoch FROM(now()-min(created_at)FILTER(WHERE state IN('pending','processing')))))FROM feed_processing_jobs").fetch_one(&state.db).await?;
    for (name, key) in [
        ("pending", "pending"),
        ("processing", "processing"),
        ("retrying", "retrying"),
        ("oldest_seconds", "oldestSeconds"),
    ] {
        if let Some(n) = queues[key].as_f64() {
            body.push_str(&format!("warcon_feed_jobs_{name} {n}\n"))
        }
    }
    Ok((
        [
            ("content-type", "text/plain; version=0.0.4; charset=utf-8"),
            ("cache-control", "no-store"),
        ],
        body,
    )
        .into_response())
}
