//! Same-round precision, change buckets, completed-minute evidence and independent episodes.
use crate::{integrity_score::num, integrity_weapons};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
pub fn precision_class(cause: &str) -> Option<&'static str> {
    Some(match cause {
        "Id.Item.AK74M" | "Id.Item.WEPN_029" | "Id.Item.M4" | "Id.Item.KH2002"
        | "Id.Item.TAR21" | "Id.Item.A91" | "Id.Item.MP9" | "Id.Item.SKS" => "automatic",
        "Id.Item.Mosin" | "Id.Item.SVDM" | "Id.Item.SV98" | "Id.Item.MK22" => "sniper",
        "Id.Item.M500" | "Id.Item.MP43" => "shotgun",
        _ => return None,
    })
}
pub fn compare_precision(rows: &[Value], id: &str) -> Vec<Value> {
    let mut population: Vec<(String, String, u64, u64)> = Vec::new();
    for row in rows {
        let valid = |key: &str| {
            row[key]
                .as_f64()
                .filter(|n| n.is_finite() && n.fract() == 0. && *n >= 0. && *n <= 9007199254740991.)
                .map(|n| n as u64)
        };
        let (Some(kills), Some(headshots)) = (valid("kills"), valid("headshots")) else {
            continue;
        };
        if kills == 0 || headshots > kills {
            continue;
        }
        let steam = row["steamId"].as_str().unwrap_or("");
        let category = row["category"].as_str().unwrap_or("");
        if let Some(previous) = population
            .iter_mut()
            .find(|r| r.0 == steam && r.1 == category)
        {
            previous.2 = previous.2.saturating_add(kills);
            previous.3 = previous.3.saturating_add(headshots);
        } else {
            population.push((steam.into(), category.into(), kills, headshots));
        }
    }
    population.iter().filter(|r|r.0==id).map(|own|{
        let peers:Vec<_>=population.iter().filter(|r|r.0!=id && r.1==own.1).collect();let peer_kills=peers.iter().map(|r|r.2).sum::<u64>();let peer_heads=peers.iter().map(|r|r.3).sum::<u64>();let rate=own.3 as f64/own.2 as f64;let average=if peer_kills>0{Some(peer_heads as f64/peer_kills as f64)}else{None};
        let less=peers.iter().filter(|r|r.3 as f64* (own.2 as f64)<own.3 as f64*r.2 as f64).count();let equal=peers.iter().filter(|r|r.3 as f64*own.2 as f64==own.3 as f64*r.2 as f64).count();
        json!({"category":own.1,"kills":own.2,"headshots":own.3,"rate":rate,"peerPlayers":peers.len(),"peerKills":peer_kills,"peerHeadshots":peer_heads,"serverRate":average,"difference":average.map(|a|rate-a),"ratio":average.filter(|a|*a!=0.).map(|a|rate/a),"percentile":if peers.is_empty(){None}else{Some((less as f64+equal as f64*0.5)/peers.len() as f64)}})
    }).collect()
}
pub fn precision_decision(row: &Value) -> &'static str {
    let (watch, high) = match row["category"].as_str() {
        Some("automatic") => (60., 70.),
        Some("sniper") => (70., 90.),
        Some("shotgun") => (30., 50.),
        _ => return "UNKNOWN",
    };
    if num(row, "kills") < 5. {
        return "UNKNOWN";
    }
    let above = |p: f64| {
        row["percentile"].as_f64().is_some_and(|n| n >= p)
            && row["difference"].as_f64().is_some_and(|n| n > 0.)
    };
    if num(row, "headshots") * 100. >= high * num(row, "kills") || above(0.95) {
        "CHEAT_LIKELY"
    } else if num(row, "headshots") * 100. >= watch * num(row, "kills") || above(0.9) {
        "SUSPICIOUS"
    } else {
        "NORMAL"
    }
}
pub fn consecutive_minutes(clocks: &[f64], clock: f64, minutes: usize, threshold: f64) -> Value {
    let end = (clock / 60.).floor() * 60.;
    let start = end - minutes as f64 * 60.;
    let windows:Vec<_>=(0..minutes).map(|i|{let from=start+i as f64*60.;let to=from+60.;let kills=clocks.iter().filter(|t|**t>=from && **t<to).count();json!({"from":from,"to":to,"kills":kills,"kpm":kills,"exceeded":kills as f64>=threshold})}).collect();
    let passed = start >= 0. && windows.iter().all(|w| w["exceeded"] == true);
    json!({"policyVersion":"consecutive-minute-v1","requiredMinutes":minutes,"threshold":threshold,"windows":windows,"passed":passed,"reason":if start<0.{"完整分钟数不足"}else if passed{"连续完整分钟均达到阈值"}else{"存在未达到阈值的分钟"}})
}
pub fn round_series(clocks: &[f64], from: f64, clock: f64) -> Vec<f64> {
    let start = (from / 15.).ceil() * 15.;
    let end = (clock / 15.).floor() * 15.;
    let count = ((end - start) / 15.).floor().max(0.);
    if !count.is_finite() || !(20. ..=5760.).contains(&count) {
        return Vec::new();
    }
    let mut series = vec![0.; count as usize];
    for t in clocks {
        if t.is_finite() && *t >= start && *t < end {
            series[((t - start) / 15.).floor() as usize] += 4.;
        }
    }
    series
}
pub fn evidence_ids(value: &Value) -> Option<Vec<String>> {
    let ids = value.as_array()?;
    if ids.is_empty() {
        return None;
    }
    ids.iter()
        .map(|id| id.as_str().filter(|s| !s.is_empty()).map(str::to_owned))
        .collect()
}
pub fn overlaps(saved: &Value, current: &[String]) -> bool {
    evidence_ids(saved).is_some_and(|ids| current.iter().any(|id| ids.contains(id)))
}
pub fn independent_episode(saved: &Value, current: &Value, separation: f64) -> bool {
    let Some(ids) = evidence_ids(&current["eventIds"]) else {
        return false;
    };
    if evidence_ids(&saved["eventIds"]).is_none() || overlaps(&saved["eventIds"], &ids) {
        return false;
    }
    if saved["roundId"].as_str().is_some_and(|s| !s.is_empty())
        && saved["roundId"] != current["roundId"]
    {
        return true;
    }
    if saved["roundId"] == current["roundId"] {
        return saved["clockTo"]
            .as_f64()
            .is_some_and(|s| num(current, "clockFrom") - s >= separation);
    }
    let time = |r: &Value| {
        r["observedAt"]
            .as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
    };
    time(saved)
        .zip(time(current))
        .is_some_and(|(a, b)| (b - a).num_milliseconds() as f64 >= separation * 1000.)
}
pub struct RoundInput<'a> {
    pub server: &'a str,
    pub steam: &'a str,
    pub instance: &'a str,
    pub match_row: i64,
    pub clock: f64,
    pub at: DateTime<Utc>,
}
pub async fn load_precision(
    db: &mut sqlx::PgConnection,
    input: &RoundInput<'_>,
    overrides: &HashMap<String, String>,
) -> crate::error::Result<Vec<Value>> {
    let rows:Vec<Value>=sqlx::query_scalar("WITH events AS (SELECT DISTINCT ON(k.event_id) k.event_id,k.killer_steam_id,k.cause,k.headshot,k.tags FROM kills k JOIN matches m ON m.id=k.match_row AND m.server_id=k.server_id WHERE k.server_id=$1 AND k.instance_id=$2 AND k.match_row=$3 AND k.ts>=m.started_at AND k.ts<=$4 AND k.event_time<=$5 AND NOT k.suicide AND NOT k.team_kill AND k.killer_steam_id IS NOT NULL AND k.killer_steam_id<>k.victim_steam_id AND (k.killer_faction IS NULL OR k.victim_faction IS NULL OR k.killer_faction<>k.victim_faction) ORDER BY k.event_id,k.ts DESC) SELECT jsonb_build_object('steamId',killer_steam_id,'cause',cause,'tags',tags,'kills',count(*)::int,'headshots',count(*) FILTER(WHERE headshot)::int) FROM events GROUP BY killer_steam_id,cause,tags").bind(input.server).bind(input.instance).bind(input.match_row).bind(input.at).bind(input.clock).fetch_all(db).await?;
    let counts: Vec<_> = rows
        .into_iter()
        .filter_map(|mut row| {
            let category = precision_class(row["cause"].as_str().unwrap_or(""))?;
            if row["tags"]
                .as_array()
                .is_some_and(|t| t.iter().any(|v| v == "WeaponMelee"))
                || integrity_weapons::classify(&row, overrides) != "INFANTRY"
            {
                return None;
            }
            row["category"] = json!(category);
            Some(row)
        })
        .collect();
    Ok(compare_precision(&counts, input.steam))
}
pub async fn load_change(
    db: &mut sqlx::PgConnection,
    input: &RoundInput<'_>,
) -> crate::error::Result<Vec<f64>> {
    let joined:Option<DateTime<Utc>>=sqlx::query_scalar("SELECT joined_at FROM player_sessions WHERE server_id=$1 AND steam_id=$2 AND left_at IS NULL AND joined_at<=$3 AND last_seen>=$3-interval '65 seconds' ORDER BY joined_at DESC LIMIT 1").bind(input.server).bind(input.steam).bind(input.at).fetch_optional(&mut *db).await?;
    let Some(joined) = joined else {
        return Ok(Vec::new());
    };
    let from = (input.clock - (input.at - joined).num_milliseconds() as f64 / 1000.).max(0.);
    let rows:Vec<(String,f32,bool,bool,String)>=sqlx::query_as("SELECT event_id,event_time,suicide,team_kill,victim_steam_id FROM kills WHERE server_id=$1 AND instance_id=$2 AND match_row=$3 AND killer_steam_id=$4 AND event_time>=$5 AND event_time<$6 AND ts<=$7 LIMIT 20001").bind(input.server).bind(input.instance).bind(input.match_row).bind(input.steam).bind(from).bind(input.clock).bind(input.at).fetch_all(db).await?;
    if rows.len() > 20000 {
        return Ok(Vec::new());
    }
    let mut seen = HashSet::new();
    let clocks: Vec<_> = rows
        .into_iter()
        .filter(|r| seen.insert(r.0.clone()) && !r.2 && !r.3 && r.4 != input.steam)
        .map(|r| r.1 as f64)
        .collect();
    Ok(round_series(&clocks, from, input.clock))
}
