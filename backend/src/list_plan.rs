use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Desired {
    pub steam_id: String,
    pub list_id: String,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stored {
    pub kind: String,
    pub steam_id: String,
    pub source_list_id: Option<String>,
    pub state: String,
    pub attempted_at: Option<DateTime<Utc>>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Add {
    pub kind: String,
    pub steam_id: String,
    pub list_id: String,
    pub reason: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reference {
    pub kind: String,
    pub steam_id: String,
}
#[derive(Default, Serialize)]
pub struct Plan {
    pub adds: Vec<Add>,
    pub removes: Vec<Reference>,
    pub confirms: Vec<Add>,
    pub deletes: Vec<Reference>,
    pub local: Vec<Reference>,
}
pub fn plan(
    now: DateTime<Utc>,
    retry_after_ms: i64,
    desired: &[Desired],
    observed: &[String],
    state: &[Stored],
) -> Plan {
    let mut result = Plan::default();
    let observed: HashSet<_> = observed.iter().collect();
    let by_id: HashMap<_, _> = state
        .iter()
        .filter(|s| s.kind == "reserve")
        .map(|s| (&s.steam_id, s))
        .collect();
    let wanted: HashSet<_> = desired.iter().map(|d| &d.steam_id).collect();
    let backed_off = |s: &Stored| {
        s.state == "failed"
            && s.attempted_at
                .is_some_and(|t| (now - t).num_milliseconds() < retry_after_ms)
    };
    for d in desired {
        let add = Add {
            kind: "reserve".into(),
            steam_id: d.steam_id.clone(),
            list_id: d.list_id.clone(),
            reason: String::new(),
        };
        if let Some(s) = by_id.get(&d.steam_id) {
            if observed.contains(&d.steam_id) {
                if s.state != "applied" || s.source_list_id.as_deref() != Some(&d.list_id) {
                    result.confirms.push(add)
                }
            } else if !backed_off(s) {
                result.adds.push(add)
            }
        } else if observed.contains(&d.steam_id) {
            result.local.push(Reference {
                kind: "reserve".into(),
                steam_id: d.steam_id.clone(),
            })
        } else {
            result.adds.push(add)
        }
    }
    // Preserve database input order; HashMap iteration is not a command ordering contract.
    for s in state.iter().filter(|s| s.kind == "reserve") {
        if wanted.contains(&s.steam_id) {
            continue;
        }
        let r = Reference {
            kind: "reserve".into(),
            steam_id: s.steam_id.clone(),
        };
        if observed.contains(&s.steam_id) {
            if !backed_off(s) {
                result.removes.push(r)
            }
        } else {
            result.deletes.push(r)
        }
    }
    result
}
pub fn already(status: u16, code: &str, message: &str) -> bool {
    code != "revision_conflict"
        && (status == 409
            || code == "already"
            || message
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .any(|s| s.eq_ignore_ascii_case("already")))
}
pub fn gone(status: u16, code: &str) -> bool {
    (status == 404 || code == "not_found") && code != "no_route"
}
pub fn unreachable(status: u16, code: &str) -> bool {
    status >= 500 || status == 429 || ["unreachable", "rate_limited"].contains(&code)
}
pub fn desired(rows: &[Value]) -> Value {
    let mut rows = rows.iter().collect::<Vec<_>>();
    rows.sort_by_key(|r| !r["serverId"].is_null());
    let mut bans = vec![];
    let mut reserved = vec![];
    let mut seen = HashSet::new();
    for r in rows {
        let kind = r["kind"].as_str().unwrap_or("");
        let id = r["steamId"].as_str().unwrap_or("");
        if !seen.insert((kind, id)) {
            continue;
        }
        if kind == "ban" {
            bans.push(json!({"steamId":id,"reason":r["reason"],"listId":r["listId"]}))
        } else {
            reserved.push(json!({"steamId":id,"listId":r["listId"],"member":false}))
        }
    }
    json!({"bans":bans,"reserved":reserved})
}
