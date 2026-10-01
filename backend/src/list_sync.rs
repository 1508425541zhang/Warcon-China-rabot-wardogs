//! Reconcile panel-managed reserved slots. Ban lists remain panel-enforced; local game bans stay untouched.
use crate::{
    actions,
    config::AppState,
    dispatcher,
    error::{ApiError, Result},
    game::{self, Client},
    list_plan::{self, Desired, Stored},
};
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
use std::{collections::HashSet, time::Duration};
pub async fn desired_for(
    state: &AppState,
    id: &str,
    org: &str,
    members_reserved: bool,
) -> Result<Value> {
    let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',e.id,'kind',l.kind,'steamId',e.steam_id,'reason',e.reason,'listId',l.id,'serverId',l.server_id,'addedBy',e.added_by,'addedByName',e.added_by_name) FROM server_lists sl JOIN lists l ON l.id=sl.list_id JOIN list_entries e ON e.list_id=l.id WHERE sl.server_id=$1 AND l.org_id=$2 AND e.removed_at IS NULL AND (e.expires_at IS NULL OR e.expires_at>now()) ORDER BY (l.server_id IS NOT NULL),e.added_at,e.id").bind(id).bind(org).fetch_all(&state.db).await?;
    let vips: Option<Value> =
        sqlx::query_scalar("SELECT value FROM site_settings WHERE key='qqVip'")
            .fetch_optional(&state.db)
            .await?;
    let vips = vips
        .as_ref()
        .and_then(|v| v["entries"].as_array())
        .into_iter()
        .flatten()
        .filter(|v| v["serverId"] == id && v["enabled"] == true)
        .collect::<Vec<_>>();
    let mut eligible = vec![];
    for row in rows {
        if row["kind"] == "ban"
            && row["addedBy"].is_null()
            && vips
                .iter()
                .any(|v| v["steamId"] == row["steamId"] && v["whitelist"] == true)
        {
            let entry = row["id"].as_str().unwrap_or("");
            let exempt=match row["addedByName"].as_str(){Some("Model A-test rule")=>sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM integrity_model_runs WHERE server_id=$1 AND steam_id=$2 AND list_entry_id=$3 AND action='QUARANTINE_24H')").bind(id).bind(row["steamId"].as_str()).bind(entry).fetch_one(&state.db).await?,Some("Community Integrity rule")=>sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM integrity_actions WHERE server_id=$1 AND list_entry_id=$2 AND reverted_at IS NULL AND source='RULE') AND NOT EXISTS(SELECT 1 FROM integrity_actions WHERE server_id=$1 AND list_entry_id=$2 AND reverted_at IS NULL AND source='REVIEW')").bind(id).bind(entry).fetch_one(&state.db).await?,_=>false};
            if exempt {
                continue;
            }
        }
        eligible.push(row)
    }
    let mut desired = list_plan::desired(&eligible);
    let banned = desired["bans"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|b| b["steamId"].as_str().map(str::to_owned))
        .collect::<HashSet<_>>();
    let mut reserved = desired["reserved"].as_array().unwrap().clone();
    let mut have = reserved
        .iter()
        .filter_map(|v| v["steamId"].as_str().map(str::to_owned))
        .collect::<HashSet<_>>();
    if members_reserved {
        let list:Option<String>=sqlx::query_scalar("SELECT l.id FROM server_lists sl JOIN lists l ON l.id=sl.list_id WHERE sl.server_id=$1 AND l.org_id=$2 AND l.kind='reserve' AND l.server_id IS NULL ORDER BY l.created_at LIMIT 1").bind(id).bind(org).fetch_optional(&state.db).await?;
        if let Some(list) = list {
            let members:Vec<String>=sqlx::query_scalar("SELECT u.steam_id FROM org_members m JOIN \"user\" u ON u.id=m.user_id WHERE m.org_id=$1 AND u.steam_id IS NOT NULL AND NOT coalesce(u.banned,false) ORDER BY m.created_at,u.id").bind(org).fetch_all(&state.db).await?;
            for steam in members {
                if !banned.contains(&steam) && have.insert(steam.clone()) {
                    reserved.push(json!({"steamId":steam,"listId":list,"member":true}))
                }
            }
        }
    }
    let own: Option<String> =
        sqlx::query_scalar("SELECT id FROM lists WHERE server_id=$1 AND kind='reserve'")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    if let Some(own) = own {
        for vip in vips {
            if vip["reserve"] == true {
                let steam = vip["steamId"].as_str().unwrap_or("");
                if !banned.contains(steam) && have.insert(steam.to_owned()) {
                    reserved.push(json!({"steamId":steam,"listId":own,"member":false}))
                }
            }
        }
    }
    desired["reserved"] = json!(reserved);
    Ok(desired)
}
fn base(id: &str, name: &str) -> Value {
    json!({"serverId":id,"serverName":name,"ok":false,"added":0,"removed":0,"failed":0,"pending":false,"error":""})
}
fn message(e: &game::Error) -> String {
    match e {
        game::Error::Api(e) => e.message.clone(),
        game::Error::Game(e) => e.message.clone(),
    }
}
async fn bookkeep(state: &AppState, id: &str, error: &str, success: bool) -> Result<()> {
    sqlx::query("INSERT INTO server_list_sync(server_id,synced_at,last_error,updated_at) VALUES($1,CASE WHEN $3 THEN now() ELSE NULL END,$2,now()) ON CONFLICT(server_id) DO UPDATE SET synced_at=CASE WHEN $3 THEN now() ELSE server_list_sync.synced_at END,last_error=$2,updated_at=now()").bind(id).bind(error).bind(success).execute(&state.db).await?;
    Ok(())
}
async fn applied(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
    steam: &str,
    list: Option<&str>,
    error: &str,
) -> Result<()> {
    sqlx::query("INSERT INTO server_list_state(server_id,kind,steam_id,source_list_id,state,error,attempted_at,updated_at) VALUES($1,'reserve',$2,$3,$4,$5,now(),now()) ON CONFLICT(server_id,kind,steam_id) DO UPDATE SET source_list_id=excluded.source_list_id,state=excluded.state,error=excluded.error,attempted_at=excluded.attempted_at,updated_at=excluded.updated_at").bind(id).bind(steam).bind(list).bind(if error.is_empty(){"applied"}else{"failed"}).bind(crate::feed::truncate(error,300)).execute(&mut **tx).await?;
    Ok(())
}
pub async fn reconcile(state: &AppState, id: &str, priority: u8) -> Result<Value> {
    let row:Option<(String,String,bool,Option<chrono::DateTime<chrono::Utc>>)>=sqlx::query_as("SELECT s.name,s.org_id,o.members_reserved,o.suspended_at FROM servers s JOIN organizations o ON o.id=s.org_id WHERE s.id=$1").bind(id).fetch_optional(&state.db).await?;
    let (name, org, members, suspended) = row.ok_or_else(ApiError::missing)?;
    let mut summary = base(id, &name);
    if suspended.is_some() {
        summary["error"] = json!("Organisation suspended.");
        return Ok(summary);
    }
    let _lane = match dispatcher::acquire(id, priority, Duration::from_secs(15)).await {
        Ok(g) => g,
        Err(e) => {
            summary["pending"] = json!(false);
            summary["error"] = json!(e.message);
            return Ok(summary);
        }
    };
    // Re-read suspension and desired state after waiting for the lane.
    let current: Option<(bool, Option<chrono::DateTime<chrono::Utc>>)> =
        sqlx::query_as("SELECT members_reserved,suspended_at FROM organizations WHERE id=$1")
            .bind(&org)
            .fetch_optional(&state.db)
            .await?;
    let Some((members_now, None)) = current else {
        summary["error"] = json!("Organisation suspended or removed.");
        return Ok(summary);
    };
    let _ = members;
    let desired = desired_for(state, id, &org, members_now).await?;
    let wanted: Vec<Desired> = serde_json::from_value(desired["reserved"].clone())
        .map_err(|_| ApiError::bad("Invalid desired reserve state."))?;
    let stored:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('kind',kind,'steamId',steam_id,'sourceListId',source_list_id,'state',state,'attemptedAt',attempted_at) FROM server_list_state WHERE server_id=$1 ORDER BY updated_at,steam_id").bind(id).fetch_all(&state.db).await?;
    let stored: Vec<Stored> = serde_json::from_value(json!(stored))
        .map_err(|_| ApiError::bad("Invalid reserve state."))?;
    let cached: Vec<String> = sqlx::query_scalar(
        "SELECT steam_id FROM server_reserved WHERE server_id=$1 ORDER BY steam_id",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    let mut plan = list_plan::plan(chrono::Utc::now(), 300000, &wanted, &cached, &stored);
    if plan.adds.is_empty()
        && plan.removes.is_empty()
        && plan.confirms.is_empty()
        && plan.deletes.is_empty()
    {
        bookkeep(state, id, "", true).await?;
        summary["ok"] = json!(true);
        return Ok(summary);
    }
    let client = match Client::for_server(state, id).await {
        Ok(c) => c,
        Err(e) => {
            let error = message(&e);
            bookkeep(state, id, &error, false).await?;
            summary["error"] = json!(error);
            return Ok(summary);
        }
    };
    let observed = async {
        let bans = actions::run(&client, "bans", &json!({})).await?;
        let reserved = actions::run(&client, "reserved", &json!({})).await?;
        Ok::<_, game::Error>((bans, reserved))
    }
    .await;
    let (bans, reserved) = match observed {
        Ok(v) => v,
        Err(e) => {
            let error = message(&e);
            bookkeep(state, id, &error, false).await?;
            summary["error"] = json!(error);
            return Ok(summary);
        }
    };
    let mut live: Vec<String> = reserved["reserved"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| {
            v.as_str()
                .filter(|s| crate::api::notes::steam_id(s).is_ok())
                .map(str::to_owned)
        })
        .collect();
    plan = list_plan::plan(chrono::Utc::now(), 300000, &wanted, &live, &stored);
    let mut via_config = false;
    let mut writable = true;
    if !plan.adds.is_empty() || !plan.removes.is_empty() {
        if let Ok(features) = actions::run(&client, "capabilities", &json!({})).await {
            via_config = features["features"]["reservedSlots"] != true;
            writable = !via_config || features["features"]["configDocument"] == true;
        }
    }
    let mut succeeded_add = vec![];
    let mut succeeded_remove = vec![];
    let mut failures: Vec<(String, Option<String>, String)> = vec![];
    let mut aborted = String::new();
    for (add, steam, list) in plan
        .removes
        .iter()
        .map(|r| (false, r.steam_id.clone(), None))
        .chain(
            plan.adds
                .iter()
                .map(|a| (true, a.steam_id.clone(), Some(a.list_id.clone()))),
        )
    {
        if !writable {
            failures.push((
                steam,
                list,
                "This server has no writable reserved-slot route or configuration document.".into(),
            ));
            continue;
        }
        let result = actions::edit_reserved(
            &client,
            &json!({"steamId":steam,"viaConfig":via_config}),
            add,
        )
        .await;
        let success = match result {
            Ok(_) => true,
            Err(game::Error::Game(ref e))
                if (add && list_plan::already(e.status, &e.code, &e.message))
                    || (!add && list_plan::gone(e.status, &e.code)) =>
            {
                true
            }
            Err(ref e) => {
                let unreachable =
                    matches!(e,game::Error::Game(e) if list_plan::unreachable(e.status,&e.code));
                if unreachable {
                    aborted = message(e);
                    break;
                }
                failures.push((steam.clone(), list.clone(), message(e)));
                false
            }
        };
        if success {
            if add {
                if !live.contains(&steam) {
                    live.push(steam.clone())
                }
                succeeded_add.push((steam, list.unwrap()));
            } else {
                live.retain(|s| s != &steam);
                succeeded_remove.push(steam)
            }
        }
    }
    let mut tx = state.db.begin().await?;
    for (steam, list) in &succeeded_add {
        applied(&mut tx, id, steam, Some(list), "").await?
    }
    for a in &plan.confirms {
        applied(&mut tx, id, &a.steam_id, Some(&a.list_id), "").await?
    }
    for (steam, list, error) in &failures {
        applied(&mut tx, id, steam, list.as_deref(), error).await?
    }
    for steam in succeeded_remove
        .iter()
        .chain(plan.deletes.iter().map(|r| &r.steam_id))
    {
        sqlx::query(
            "DELETE FROM server_list_state WHERE server_id=$1 AND kind='reserve' AND steam_id=$2",
        )
        .bind(id)
        .bind(steam)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("DELETE FROM server_reserved WHERE server_id=$1 AND NOT(steam_id=ANY($2))")
        .bind(id)
        .bind(&live)
        .execute(&mut *tx)
        .await?;
    for steam in &live {
        sqlx::query("INSERT INTO server_reserved(server_id,steam_id,seen_at) VALUES($1,$2,now()) ON CONFLICT(server_id,steam_id) DO UPDATE SET seen_at=excluded.seen_at").bind(id).bind(steam).execute(&mut *tx).await?;
    }
    let bans = bans["bans"].as_array().cloned().unwrap_or_default();
    let ban_ids = bans
        .iter()
        .filter_map(|b| {
            b["steamId"]
                .as_str()
                .filter(|s| crate::api::notes::steam_id(s).is_ok())
                .map(str::to_owned)
        })
        .collect::<Vec<_>>();
    sqlx::query("DELETE FROM server_bans WHERE server_id=$1 AND NOT(steam_id=ANY($2))")
        .bind(id)
        .bind(&ban_ids)
        .execute(&mut *tx)
        .await?;
    for b in &bans {
        if let Some(steam) = b["steamId"]
            .as_str()
            .filter(|s| crate::api::notes::steam_id(s).is_ok())
        {
            sqlx::query("INSERT INTO server_bans(server_id,steam_id,reason,banned_by,banned_at_utc,seen_at) VALUES($1,$2,$3,$4,$5,now()) ON CONFLICT(server_id,steam_id) DO UPDATE SET reason=excluded.reason,banned_by=excluded.banned_by,banned_at_utc=excluded.banned_at_utc,seen_at=excluded.seen_at").bind(id).bind(steam).bind(b["reason"].as_str().unwrap_or("")).bind(b["bannedBy"].as_str().unwrap_or("")).bind(b["bannedAtUtc"].as_str().unwrap_or("")).execute(&mut *tx).await?;
        }
    }
    sqlx::query("INSERT INTO server_list_sync(server_id,synced_at,last_error,updated_at) VALUES($1,now(),$2,now()) ON CONFLICT(server_id) DO UPDATE SET synced_at=excluded.synced_at,last_error=excluded.last_error,updated_at=excluded.updated_at").bind(id).bind(&aborted).execute(&mut *tx).await?;
    if !succeeded_add.is_empty()
        || !succeeded_remove.is_empty()
        || !failures.is_empty()
        || !aborted.is_empty()
    {
        sqlx::query("INSERT INTO audit_log(actor_name,org_id,server_id,server_name,category,action,target,detail,outcome,status,message) VALUES('list sync',$1,$2,$3,'system','lists.sync',$4,$5,$6,$7,$8)").bind(&org).bind(id).bind(&name).bind(&org).bind(json!({"added":succeeded_add.iter().map(|a|format!("reserve:{}",a.0)).collect::<Vec<_>>(),"removed":succeeded_remove,"failed":failures.iter().map(|(steam,_,error)|json!({"kind":"reserve","steamId":steam,"error":error})).collect::<Vec<_>>()})).bind(if failures.is_empty()&&aborted.is_empty(){"ok"}else{"error"}).bind(if failures.is_empty()&&aborted.is_empty(){200}else{502}).bind(crate::feed::truncate(&aborted,1000)).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    summary["ok"] = json!(true);
    summary["added"] = json!(succeeded_add.len());
    summary["removed"] = json!(succeeded_remove.len());
    summary["failed"] = json!(failures.len());
    summary["error"] = json!(aborted);
    Ok(summary)
}
pub async fn sync_org(state: &AppState, org: &str) -> Result<Value> {
    expire_entries(state).await?;
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT id,name FROM servers WHERE org_id=$1 ORDER BY sort_order,name")
            .bind(org)
            .fetch_all(&state.db)
            .await?;
    let limit = std::sync::Arc::new(tokio::sync::Semaphore::new(4));
    let mut tasks = vec![];
    for (id, name) in rows {
        let state = state.clone();
        let permit = limit.clone();
        let job_id = id.clone();
        let job = tokio::spawn(async move {
            let _permit = permit
                .acquire_owned()
                .await
                .map_err(|_| ApiError::bad("Sync queue stopped."))?;
            reconcile(&state, &job_id, 0).await
        });
        tasks.push((id, name, job))
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let mut summaries = vec![];
    for (id, name, mut job) in tasks {
        let summary = match tokio::time::timeout_at(deadline, &mut job).await {
            Ok(Ok(Ok(v))) => v,
            Ok(_) => {
                let mut v = base(&id, &name);
                v["error"] = json!("List sync failed.");
                v
            }
            Err(_) => {
                let mut v = base(&id, &name);
                v["pending"] = json!(true);
                v
            }
        };
        summaries.push(summary)
    }
    Ok(json!({"servers":summaries}))
}

/// Lift expired entries atomically with their audit records. Never delete their history.
pub async fn expire_entries(state: &AppState) -> Result<Value> {
    let mut tx = state.db.begin().await?;
    let rows:Vec<(String,String)> = sqlx::query_as("UPDATE list_entries SET removed_at=now(),removed_by_name='expiry',removal='expired' WHERE removed_at IS NULL AND expires_at<=now() RETURNING list_id,steam_id").fetch_all(&mut *tx).await?;
    let list_ids = rows
        .iter()
        .map(|r| r.0.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let lists:Vec<(String,String,String,String)>=sqlx::query_as("UPDATE lists SET updated_at=now() WHERE id=ANY($1) RETURNING id,org_id,kind,(SELECT name FROM organizations WHERE id=lists.org_id)").bind(&list_ids).fetch_all(&mut *tx).await?;
    for (list, org, kind, name) in &lists {
        let ids = rows
            .iter()
            .filter(|r| r.0 == *list)
            .map(|r| r.1.clone())
            .collect::<Vec<_>>();
        sqlx::query("INSERT INTO audit_log(actor_name,org_id,category,action,target,detail,outcome,message) VALUES('list sync',$1,'system','list.expire',$2,$3,'ok',$4)").bind(org).bind(ids.join(", ")).bind(json!({"orgId":org,"org":name,"steamIds":ids})).bind(format!("{} {kind} entries expired in {name}",ids.len())).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(
        json!({"lifted":rows.len(),"orgIds":lists.iter().map(|r|r.1.clone()).collect::<HashSet<_>>()}),
    )
}
