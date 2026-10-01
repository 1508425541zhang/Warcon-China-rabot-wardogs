//! Feed-derived match columns. Scoreboard counters remain the source of total kills/deaths.
use crate::error::Result;
use serde_json::{Value, json};
use std::collections::HashMap;
#[derive(Default)]
struct Record {
    headshots: i32,
    team_kills: i32,
    suicides: i32,
    vehicle_kills: i32,
    longest: Option<f64>,
    kill_streak: i32,
    death_streak: i32,
    kill_run: i32,
    death_run: i32,
}
pub fn record(kills: &[Value]) -> Value {
    let mut out = HashMap::<String, Record>::new();
    for kill in kills {
        if let Some(killer) = kill["killerSteamId"]
            .as_str()
            .filter(|_| kill["suicide"] != true)
        {
            let r = out.entry(killer.into()).or_default();
            if kill["teamKill"] == true {
                r.team_kills += 1
            } else {
                if kill["headshot"] == true {
                    r.headshots += 1
                }
                let cause = kill["cause"].as_str().unwrap_or("").to_ascii_lowercase();
                if cause.starts_with("vehicle.") || cause.starts_with("id.vehicle.weaponextension.")
                {
                    r.vehicle_kills += 1
                }
                if let Some(m) = kill["distanceM"].as_f64() {
                    if r.longest.is_none_or(|old| m > old) {
                        r.longest = Some(m)
                    }
                }
                r.kill_run += 1;
                r.death_run = 0;
                r.kill_streak = r.kill_streak.max(r.kill_run);
            }
        }
        if let Some(victim) = kill["victimSteamId"].as_str() {
            let r = out.entry(victim.into()).or_default();
            if kill["suicide"] == true {
                r.suicides += 1
            }
            r.death_run += 1;
            r.kill_run = 0;
            r.death_streak = r.death_streak.max(r.death_run);
        }
    }
    Value::Object(out.into_iter().map(|(id,r)|(id,json!({"headshots":r.headshots,"teamKills":r.team_kills,"suicides":r.suicides,"vehicleKills":r.vehicle_kills,"longestM":r.longest,"killStreak":r.kill_streak,"deathStreak":r.death_streak}))).collect())
}
pub async fn enrich(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    server: &str,
    match_id: i64,
) -> Result<()> {
    let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('killerSteamId',k.killer_steam_id,'victimSteamId',k.victim_steam_id,'headshot',k.headshot,'suicide',k.suicide,'teamKill',k.team_kill,'cause',k.cause,'distanceM',k.distance_m) FROM kills k JOIN matches m ON m.id=$2 WHERE k.server_id=$1 AND k.match_row=$2 AND k.ts>=m.started_at-interval '120 seconds' ORDER BY k.event_time,k.ts").bind(server).bind(match_id).fetch_all(&mut **tx).await?;
    let data = record(&rows);
    let rows=json!(data.as_object().unwrap().iter().map(|(id,r)|json!({"steam_id":id,"headshots":r["headshots"],"team_kills":r["teamKills"],"suicides":r["suicides"],"vehicle_kills":r["vehicleKills"],"longest_m":r["longestM"],"kill_streak":r["killStreak"],"death_streak":r["deathStreak"]})).collect::<Vec<_>>());
    sqlx::query("UPDATE match_players m SET headshots=p.headshots,team_kills=p.team_kills,suicides=p.suicides,vehicle_kills=p.vehicle_kills,longest_m=p.longest_m,kill_streak=p.kill_streak,death_streak=p.death_streak FROM jsonb_to_recordset($2) p(steam_id text,headshots int,team_kills int,suicides int,vehicle_kills int,longest_m real,kill_streak int,death_streak int) WHERE m.match_id=$1 AND m.steam_id=p.steam_id").bind(match_id).bind(rows).execute(&mut **tx).await?;
    Ok(())
}
