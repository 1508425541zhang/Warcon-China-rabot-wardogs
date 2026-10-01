//! Permission-scoped player views; raw SQL values never leave the API without a DTO.
use crate::{
    auth::{Actor, ServerScope},
    config::AppState,
    error::Result,
    http::{integer, string},
};
use chrono::Utc;
use serde_json::{Value, json};
use std::collections::HashMap;
pub async fn visible(
    state: &AppState,
    actor: &Actor,
    org: &str,
) -> Result<(Vec<String>, HashMap<String, String>)> {
    let rows = crate::api::servers::accessible(state, actor, Some(org)).await?;
    let names = rows
        .iter()
        .map(|s| {
            (
                s["id"].as_str().unwrap().to_owned(),
                s["name"].as_str().unwrap().to_owned(),
            )
        })
        .collect::<HashMap<_, _>>();
    Ok((
        rows.iter()
            .map(|s| s["id"].as_str().unwrap().to_owned())
            .collect(),
        names,
    ))
}
pub async fn seen(
    state: &AppState,
    org: &str,
    ids: &[String],
    q: &HashMap<String, String>,
    limit: i64,
    offset: i64,
    steam: bool,
) -> Result<Value> {
    if ids.is_empty() {
        return Ok(json!({"players":[],"total":0}));
    }
    let get = |k: &str| q.get(k).map(String::as_str).unwrap_or("");
    let sort = match get("sort") {
        "firstSeen" => "first_seen",
        "minutes" => "minutes",
        "sessions" => "sessions",
        "kills" => "kills",
        "deaths" => "deaths",
        "name" => "lower(name)",
        _ => "last_seen",
    };
    let dir = if get("dir") == "asc" || get("dir") != "desc" && sort == "lower(name)" {
        "ASC"
    } else {
        "DESC"
    };
    let sql =
        include_str!("../sql/seen.sql").replace("__ORDER__", &format!("{sort} {dir} NULLS LAST"));
    let search = string(&json!(get("q")), 100);
    let pattern = format!(
        "%{}%",
        search
            .chars()
            .flat_map(|c| if "\\%_".contains(c) {
                vec!['\\', c]
            } else {
                vec![c]
            })
            .collect::<String>()
    );
    let filter = string(&json!(get("server")), 100);
    let rows: Vec<Value> = sqlx::query_scalar(&sql)
        .bind(ids)
        .bind(org)
        .bind(search)
        .bind(&filter)
        .bind(integer(&json!(get("since")), 0, 0, 3650) as i32)
        .bind(pattern)
        .bind(!filter.is_empty() && ids.contains(&filter))
        .bind(get("flag"))
        .bind(limit)
        .bind(offset)
        .fetch_all(&state.db)
        .await?;
    let total = rows.first().map(|r| r["total"].clone()).unwrap_or(json!(0));
    let mut players = vec![];
    for r in rows {
        let mut p = json!({"steamId":r["steam_id"],"name":r["name"],"aliases":r["names"].as_array().into_iter().flatten().filter(|n|**n!=r["name"]).cloned().collect::<Vec<_>>(),"firstSeen":r["first_seen"],"lastSeen":r["last_seen"],"sessions":r["sessions"],"minutes":r["minutes"],"kills":r["kills"],"deaths":r["deaths"],"servers":r["servers"],"online":r["online"],"lastServerId":r["last_server_id"],"lastServerName":r["last_server_name"].as_str().unwrap_or(""),"banned":if r["org_banned"]==true{json!("org")}else if r["server_banned"]==true{json!("server")}else{Value::Null},"watched":r["watched"],"steam":null});
        if steam {
            let profile:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('persona',persona,'avatar',avatar)FROM steam_profiles WHERE steam_id=$1").bind(r["steam_id"].as_str()).fetch_optional(&state.db).await?;
            p["steam"] = profile.unwrap_or(Value::Null)
        }
        players.push(p)
    }
    if steam {
        crate::steam::request_refresh(
            state,
            &players
                .iter()
                .filter_map(|p| p["steamId"].as_str().map(str::to_owned))
                .collect::<Vec<_>>(),
        )
        .await?;
    }
    Ok(json!({"players":players,"total":total}))
}
pub async fn combat(
    state: &AppState,
    ids: &[String],
    names: &HashMap<String, String>,
    steam: &str,
    recent: bool,
) -> Result<Value> {
    if ids.is_empty() {
        return Ok(Value::Null);
    }
    let feed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM servers WHERE id=ANY($1)AND feed_token_hash IS NOT NULL)",
    )
    .bind(ids)
    .fetch_one(&state.db)
    .await?;
    let summary:Value=sqlx::query_scalar("SELECT jsonb_build_object('kills',count(*)FILTER(WHERE killer_steam_id=$2 AND NOT suicide),'deaths',count(*)FILTER(WHERE victim_steam_id=$2),'headshots',count(*)FILTER(WHERE killer_steam_id=$2 AND headshot AND NOT suicide),'teamKills',count(*)FILTER(WHERE killer_steam_id=$2 AND team_kill),'teamKilled',count(*)FILTER(WHERE victim_steam_id=$2 AND team_kill),'suicides',count(*)FILTER(WHERE victim_steam_id=$2 AND suicide),'avgDistanceM',floor(avg(distance_m)FILTER(WHERE killer_steam_id=$2 AND NOT suicide)+0.5),'longestM',floor(max(distance_m)FILTER(WHERE killer_steam_id=$2 AND NOT suicide)+0.5))FROM kills WHERE server_id=ANY($1)AND(killer_steam_id=$2 OR victim_steam_id=$2)").bind(ids).bind(steam).fetch_one(&state.db).await?;
    if !feed && summary["kills"] == 0 && summary["deaths"] == 0 {
        return Ok(Value::Null);
    }
    let mut out = summary;
    let queries = [
        (
            "causes",
            "SELECT jsonb_build_object('cause',cause,'kills',count(*))FROM kills WHERE server_id=ANY($1)AND killer_steam_id=$2 AND NOT suicide AND cause IS NOT NULL GROUP BY cause ORDER BY count(*)DESC LIMIT 8",
        ),
        (
            "victims",
            "SELECT jsonb_build_object('steamId',victim_steam_id,'name',max(victim_name),'kills',count(*))FROM kills WHERE server_id=ANY($1)AND killer_steam_id=$2 AND NOT suicide GROUP BY victim_steam_id ORDER BY count(*)DESC LIMIT 5",
        ),
        (
            "nemeses",
            "SELECT jsonb_build_object('steamId',killer_steam_id,'name',max(killer_name),'deaths',count(*))FROM kills WHERE server_id=ANY($1)AND victim_steam_id=$2 AND killer_steam_id IS NOT NULL AND NOT suicide GROUP BY killer_steam_id ORDER BY count(*)DESC LIMIT 5",
        ),
    ];
    for (key, sql) in queries {
        out[key] = json!(
            sqlx::query_scalar::<_, Value>(sql)
                .bind(ids)
                .bind(steam)
                .fetch_all(&state.db)
                .await?
        )
    }
    if recent {
        let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(k)FROM kills k WHERE server_id=ANY($1)AND(killer_steam_id=$2 OR victim_steam_id=$2)ORDER BY ts DESC LIMIT 25").bind(ids).bind(steam).fetch_all(&state.db).await?;
        out["recent"] = json!(
            rows.iter()
                .map(|k| {
                    let mut v = crate::api::kills::view(k.clone());
                    let id = k["server_id"].as_str().unwrap();
                    v["serverId"] = json!(id);
                    v["serverName"] = json!(names.get(id).map(String::as_str).unwrap_or(id));
                    v
                })
                .collect::<Vec<_>>()
        )
    }
    Ok(out)
}
pub fn steam_view(row: Value) -> Value {
    if row.is_null() {
        return row;
    }
    let mut v = crate::ai_evidence::row(row);
    v["accountAgeDays"] = json!(crate::player_risk::account_age(
        &v["accountCreatedAt"],
        Utc::now().timestamp_millis()
    ));
    if let Some(o) = v.as_object_mut() {
        o.remove("steamId");
        o.remove("friendsCheckedAt");
    }
    v
}
pub async fn dossier(
    state: &AppState,
    actor: &Actor,
    scope: &ServerScope,
    steam: &str,
) -> Result<Value> {
    let (ids, names) = visible(state, actor, &scope.org_id).await?;
    let summary:Value=sqlx::query_scalar("SELECT jsonb_build_object('sessions',count(*),'minutes',floor(coalesce(sum(extract(epoch FROM(coalesce(left_at,now())-joined_at)))/60,0)+0.5),'firstSeen',min(joined_at),'lastSeen',max(last_seen))FROM player_sessions WHERE steam_id=$1 AND server_id=ANY($2)").bind(steam).bind(&ids).fetch_one(&state.db).await?;
    let mut per:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('serverId',server_id,'sessions',count(*),'minutes',floor(sum(extract(epoch FROM(coalesce(left_at,now())-joined_at)))/60+0.5),'lastSeen',max(last_seen))FROM player_sessions WHERE steam_id=$1 AND server_id=ANY($2)GROUP BY server_id ORDER BY max(last_seen)DESC").bind(steam).bind(&ids).fetch_all(&state.db).await?;
    let records:Vec<(String,i64,i64)>=sqlx::query_as("SELECT p.server_id,sum(p.kills)::bigint,sum(p.deaths)::bigint FROM match_players p JOIN matches m ON m.id=p.match_id WHERE p.steam_id=$1 AND p.server_id=ANY($2)AND m.ended_at IS NOT NULL GROUP BY p.server_id").bind(steam).bind(&ids).fetch_all(&state.db).await?;
    let mut summary = summary;
    summary["kills"] = json!(records.iter().map(|r| r.1).sum::<i64>());
    summary["deaths"] = json!(records.iter().map(|r| r.2).sum::<i64>());
    for p in &mut per {
        let id = p["serverId"].as_str().unwrap().to_owned();
        let r = records.iter().find(|r| r.0 == id);
        p["serverName"] = json!(names.get(&id).unwrap_or(&id));
        p["kills"] = json!(r.map(|r| r.1).unwrap_or(0));
        p["deaths"] = json!(r.map(|r| r.2).unwrap_or(0))
    }
    let recent:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(s)||jsonb_build_object('minutes',floor(extract(epoch FROM(coalesce(left_at,now())-joined_at))/60+0.5),'seedMinutes',floor(seed_seconds::float8/60+0.5))FROM player_sessions s WHERE steam_id=$1 AND server_id=ANY($2)ORDER BY last_seen DESC LIMIT 25").bind(steam).bind(&ids).fetch_all(&state.db).await?;
    let aliases:Vec<String>=sqlx::query_scalar("SELECT name FROM player_sessions WHERE steam_id=$1 AND server_id=ANY($2)GROUP BY name ORDER BY max(last_seen)DESC LIMIT 10").bind(steam).bind(&ids).fetch_all(&state.db).await?;
    let name = aliases
        .first()
        .filter(|s| !s.is_empty())
        .map(String::as_str)
        .unwrap_or(steam);
    let inputs = crate::player_risk::inputs(
        state,
        &scope.org_id,
        &ids,
        None,
        &[json!({"steamId":steam,"name":name})],
        true,
    )
    .await?;
    let mut input = inputs.get(steam).cloned().unwrap_or(Value::Null);
    let admin = scope.caps.iter().any(|s| s == "players.notes.manage");
    let staff = admin || scope.caps.iter().any(|s| s == "players.notes");
    if !staff && input["watched"].is_object() {
        input["watched"]["reason"] = json!("");
    }
    let mut mark = sqlx::query_scalar::<_, Value>(
        "SELECT to_jsonb(m)FROM player_marks m WHERE org_id=$1 AND steam_id=$2",
    )
    .bind(&scope.org_id)
    .bind(steam)
    .fetch_optional(&state.db)
    .await?
    .map(crate::ai_evidence::row)
    .unwrap_or(json!({"watched":false}));
    if !staff {
        mark["reason"] = json!("");
        mark["updatedByName"] = json!("")
    }
    mark.as_object_mut().unwrap().remove("steamId");
    mark.as_object_mut().unwrap().remove("orgId");
    let notes: Vec<Value> = if staff {
        sqlx::query_scalar("SELECT jsonb_build_object('id',id,'authorId',author_id,'authorName',author_name,'body',body,'createdAt',created_at,'deletable',$3 OR author_id=$4)FROM player_notes WHERE org_id=$1 AND steam_id=$2 ORDER BY id DESC LIMIT 100").bind(&scope.org_id).bind(steam).bind(admin).bind(&actor.id).fetch_all(&state.db).await?
    } else {
        vec![]
    };
    let visibility = crate::api::activity::visibility(state, actor).await?;
    let actions =
        crate::api::activity::target(state, &visibility, &scope.org_id, &ids, steam, 50).await?;
    let role = crate::api::lists::role_for(state, actor, &scope.org_id).await?;
    let membership =
        crate::api::lists::membership(state, &scope.org_id, steam, role.as_ref()).await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*)FROM servers WHERE org_id=$1")
        .bind(&scope.org_id)
        .fetch_one(&state.db)
        .await?;
    let online = recent.iter().find(|r| r["left_at"].is_null()).map(|r| {
        let id = r["server_id"].as_str().unwrap();
        json!({"serverId":id,"serverName":names.get(id).map(String::as_str).unwrap_or("")})
    });
    let recent: Vec<_> = recent
        .into_iter()
        .map(|r| {
            let mut v = crate::ai_evidence::row(r);
            let id = v["serverId"].as_str().unwrap();
            v["serverName"] = json!(names.get(id).map(String::as_str).unwrap_or(id));
            v.as_object_mut().unwrap().remove("steamId");
            v
        })
        .collect();
    let profile: Option<Value> =
        sqlx::query_scalar("SELECT to_jsonb(p)FROM steam_profiles p WHERE steam_id=$1")
            .bind(steam)
            .fetch_optional(&state.db)
            .await?;
    crate::steam::request_refresh(state, &[steam.to_owned()]).await?;
    Ok(
        json!({"steamId":steam,"name":name,"names":aliases,"online":online,"orgServerCount":count,"orgLists":membership,"steamEnabled":std::env::var("STEAM_API_KEY").is_ok_and(|s|!s.is_empty()),"steam":steam_view(profile.unwrap_or(Value::Null)),"risk":crate::player_risk::assess(&input,Utc::now().timestamp_millis()),"watch":mark,"bannedOn":input["bannedOn"],"combat":combat(state,&ids,&names,steam,true).await?,"summary":summary,"perServer":per,"recent":recent,"notes":notes,"actions":actions}),
    )
}
