use crate::{actions, ban_message, config::AppState, error::Result, game, list_sync};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
/// Run on the observation's held server lane, with its current player list.
/// Re-read subscriptions, expiry, bans and VIP exemptions before every delivery.
pub async fn enforce(
    state: &AppState,
    client: &game::Client,
    id: &str,
    present: &[String],
    retry: &mut HashMap<String, Instant>,
) -> Result<usize> {
    let org:Option<(String,String,String,bool,bool)>=sqlx::query_as("SELECT o.id,s.name,o.ban_message,o.members_reserved,o.suspended_at IS NOT NULL FROM servers s JOIN organizations o ON o.id=s.org_id WHERE s.id=$1").bind(id).fetch_optional(&state.db).await?;
    let Some((org, name, template, members, false)) = org else {
        return Ok(0);
    };
    let desired = list_sync::desired_for(state, id, &org, members).await?;
    let bans = desired["bans"].as_array().unwrap();
    retry.retain(|steam, deadline| *deadline > Instant::now() && present.contains(steam));
    let mut kicked = 0;
    for steam in present {
        if retry.contains_key(steam) {
            continue;
        }
        let Some(ban) = bans.iter().find(|b| b["steamId"] == *steam) else {
            continue;
        };
        let entry:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('entryId',e.id,'reason',e.reason,'addedByName',e.added_by_name,'addedAt',e.added_at,'expiresAt',e.expires_at) FROM server_lists sl JOIN list_entries e ON e.list_id=sl.list_id JOIN lists l ON l.id=e.list_id JOIN organizations o ON o.id=l.org_id WHERE sl.server_id=$1 AND e.list_id=$2 AND e.steam_id=$3 AND e.removed_at IS NULL AND (e.expires_at IS NULL OR e.expires_at>now()) AND o.suspended_at IS NULL LIMIT 1").bind(id).bind(ban["listId"].as_str()).bind(steam).fetch_optional(&state.db).await?;
        let Some(entry) = entry else { continue };
        let facts: ban_message::Facts = serde_json::from_value(entry.clone())
            .map_err(|_| crate::error::ApiError::bad("Invalid ban facts."))?;
        let reason = ban_message::render(&template, &facts);
        // Refresh automatic-ban exemptions immediately before acting, not from a poller's stale copy.
        let fresh = list_sync::desired_for(state, id, &org, members).await?;
        if !fresh["bans"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b["steamId"] == *steam && b["listId"] == ban["listId"])
        {
            continue;
        }
        let result = actions::run(client, "kick", &json!({"steamId":steam,"reason":reason})).await;
        let error = match result {
            Ok(_) => {
                kicked += 1;
                String::new()
            }
            Err(game::Error::Game(e)) if crate::list_plan::gone(e.status, &e.code) => continue,
            Err(game::Error::Game(e)) if crate::list_plan::unreachable(e.status, &e.code) => break,
            Err(game::Error::Game(e)) => e.message,
            Err(game::Error::Api(_)) => break,
        };
        retry.insert(steam.clone(), Instant::now() + Duration::from_secs(30));
        let mut tx = state.worker_transaction().await?;
        sqlx::query("INSERT INTO audit_log(actor_name,server_id,server_name,org_id,category,action,target,outcome,status,message,detail) VALUES('ban list',$1,$2,$3,'system','ban.enforce',$4,$5,$6,$7,$8)").bind(id).bind(&name).bind(&org).bind(steam).bind(if error.is_empty(){"ok"}else{"error"}).bind(if error.is_empty(){200}else{502}).bind(if error.is_empty(){"Banned player removed".into()}else{format!("Could not remove a banned player: {}",crate::feed::truncate(&error,500))}).bind(json!({"banId":ban_message::uid(&facts.entry_id)})).execute(&mut *tx).await?;
        tx.commit().await?;
    }
    Ok(kicked)
}
