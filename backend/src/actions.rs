//! Native action registry. Compound actions hold one lane for their entire operation.
use crate::{
    config_document as ini,
    error::ApiError,
    game::{Client, Error, GameError, Result},
    http::{integer, string, truthy},
};
use serde_json::{Value, json};
use std::collections::HashMap;
pub const NAMES: &[&str] = &[
    "capabilities",
    "status",
    "health",
    "serverId",
    "players",
    "maps",
    "lightings",
    "experiences",
    "alternators",
    "catalog",
    "rotation",
    "bans",
    "reserved",
    "sponsor",
    "serverLog",
    "config",
    "broadcast",
    "whisper",
    "kick",
    "kill",
    "changeTeam",
    "endMatch",
    "restartMatch",
    "changeMap",
    "setWeather",
    "setNextMap",
    "rotationAdd",
    "rotationRemove",
    "rotationMove",
    "rotationReorder",
    "ban",
    "unban",
    "reservedAdd",
    "reservedRemove",
    "rotationSave",
    "rotationSettings",
    "settings",
    "configValidate",
    "configApply",
    "raw",
];
pub fn definition(name: &str) -> Option<(&'static str, bool)> {
    Some(match name {
        "capabilities" | "status" | "health" | "serverId" | "players" | "maps" | "lightings"
        | "experiences" | "alternators" | "catalog" | "rotation" | "bans" | "reserved"
        | "sponsor" => ("server.view", false),
        "serverLog" => ("audit.read", false),
        "config" | "configValidate" => ("config.apply", false),
        "broadcast" | "whisper" => ("chat.send", true),
        "kick" | "kill" | "changeTeam" => ("players.moderate", true),
        "endMatch" | "restartMatch" | "changeMap" | "setWeather" | "setNextMap" => {
            ("match.control", true)
        }
        "rotationAdd" | "rotationRemove" | "rotationMove" | "rotationReorder" => {
            ("rotation.edit", true)
        }
        "ban" | "unban" => ("bans.manage", true),
        "reservedAdd" | "reservedRemove" => ("slots.manage", true),
        "rotationSave" | "rotationSettings" => ("rotation.save", true),
        "settings" | "configApply" => ("config.apply", true),
        "raw" => ("rcon.raw", true),
        _ => return None,
    })
}
fn s(p: &Value, key: &str, max: usize) -> String {
    string(&p[key], max)
}
fn default(p: &Value, key: &str, fallback: Value) -> Value {
    p.get(key)
        .filter(|v| !v.is_null())
        .cloned()
        .unwrap_or(fallback)
}
fn array(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}
fn steam(p: &Value) -> Result<String> {
    let id = s(p, "steamId", 32);
    crate::api::notes::steam_id(&id)?;
    Ok(id)
}
fn required(p: &Value, key: &str, max: usize) -> Result<String> {
    let v = s(p, key, max);
    if v.is_empty() {
        return Err(ApiError::bad(format!("{key} is required.")).into());
    }
    Ok(v)
}
fn encoded(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char)
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
pub fn map_selection(p: &Value) -> Result<Value> {
    let mut body = json!({"map":required(p,"map",100)?});
    let values = array(&p["experiences"])
        .iter()
        .map(|v| string(v, 100))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    if !array(&p["experiences"]).is_empty() {
        body["experiences"] = json!(values)
    }
    if truthy(&p["lighting"]) {
        body["lighting"] = json!(s(p, "lighting", 100))
    }
    if truthy(&p["zoneAlternator"]) && p["zoneAlternator"] != "None" {
        body["zoneAlternator"] = json!(s(p, "zoneAlternator", 200))
    }
    Ok(body)
}
fn same_selection(a: &Value, b: &Value) -> bool {
    let key = |v: &Value| {
        if !truthy(v) || v == "None" {
            String::new()
        } else {
            string(v, usize::MAX)
        }
    };
    let set = |v: &Value| {
        let mut a = array(v)
            .iter()
            .map(|v| string(v, usize::MAX))
            .collect::<Vec<_>>();
        a.sort();
        a.join("+")
    };
    a["map"] == b["map"]
        && set(&a["experiences"]) == set(&b["experiences"])
        && key(&a["lighting"]) == key(&b["lighting"])
        && key(&a["zoneAlternator"]) == key(&b["zoneAlternator"])
}
pub fn rotation_shape(d: Value) -> Value {
    let entries = array(&d["entries"]);
    json!({"enabled":truthy(&d["enabled"]),"mode":if truthy(&d["mode"]){string(&d["mode"],100).to_lowercase()}else{"ordered".into()},"nowIndex":entries.iter().position(|e|e["status"]=="now").map(|i|i as i64).unwrap_or(-1),"nextIndex":entries.iter().position(|e|e["status"]=="next").map(|i|i as i64).unwrap_or(-1),"entries":entries.iter().map(|e|json!({"map":e["map"],"experiences":default(e,"experiences",json!([])),"lighting":default(e,"lighting",json!("")),"zoneAlternator":default(e,"zoneAlternator",json!("")),"denied":truthy(&e["denied"]),"status":default(e,"status",json!(""))})).collect::<Vec<_>>()})
}
pub fn status_shape(d: Value, raw: bool) -> Value {
    let mut r = json!({"serverName":default(&d,"serverName",json!("")),"map":default(&d,"map",json!("")),"experiences":default(&d,"experiences",json!([])),"lighting":default(&d,"lighting",json!("")),"alternator":default(&d,"alternator",json!("")),"scoreTick":d["scoreTick"]["current"],"scoreTickMin":d["scoreTick"]["min"],"scoreTickMax":d["scoreTick"]["max"],"scoreCap":d["scoreCap"],"matchSeconds":d["matchSeconds"],"playerCount":default(&d["players"],"current",json!(0)),"maxPlayers":default(&d["players"],"max",json!(0)),"scores":array(&d["factionScores"]).iter().map(|f|json!({"name":f["name"],"colorHex":f["colorHex"],"score":f["score"]})).collect::<Vec<_>>(),"rotationNow":crate::http::js_number(&d["rotation"]["nowIndex"]).filter(|_|!d["rotation"]["nowIndex"].is_null()).unwrap_or(-1.),"rotationNext":crate::http::js_number(&d["rotation"]["nextIndex"]).filter(|_|!d["rotation"]["nextIndex"].is_null()).unwrap_or(-1.)});
    if raw {
        r["raw"] = d
    }
    r
}
pub fn players_shape(d: &Value) -> Result<Value> {
    let players = d["players"].as_array().ok_or_else(|| {
        Error::Game(GameError {
            status: 502,
            code: "bad_response".into(),
            message: "The server did not return a player list.".into(),
            body: Value::Null,
            retry_after_ms: 0,
        })
    })?;
    Ok(
        json!({"players":players.iter().map(|p|json!({"name":p["name"],"steamId":p["steamId"],"faction":p["faction"],"kills":default(p,"kills",json!(0)),"deaths":default(p,"deaths",json!(0)),"cash":default(p,"cash",json!(0)),"ping":p.get("pingMs").filter(|v|!v.is_null()).or(p.get("ping")).cloned().unwrap_or(Value::Null)})).collect::<Vec<_>>()}),
    )
}
fn normalized_route(route: &str) -> String {
    route
        .split_whitespace()
        .map(|s| {
            s.split('/')
                .map(|s| {
                    if s.starts_with(':') || (s.starts_with('{') && s.ends_with('}')) {
                        "*"
                    } else {
                        s
                    }
                })
                .collect::<Vec<_>>()
                .join("/")
        })
        .collect::<Vec<_>>()
        .join(" ")
}
pub fn capabilities_shape(d: Value) -> Value {
    let routes = array(&d["routes"])
        .iter()
        .filter_map(Value::as_str)
        .map(normalized_route)
        .collect::<Vec<_>>();
    let has = |r: &str| routes.iter().any(|s| s == r);
    json!({"routes":default(&d,"routes",json!([])),"features":{"changeTeam":has("PATCH /v1/players/*"),"configDocument":has("PUT /v1/config")&&truthy(&d["config"]["writable"]),"reservedSlots":has("POST /v1/reserved-slots"),"rotationEdit":has("POST /v1/rotation/entries")&&has("POST /v1/rotation/entries/*/move"),"rotationSave":has("POST /v1/rotation/save"),"liveSettings":has("PATCH /v1/settings"),"serverId":has("GET /v1/server-id")},"raw":d})
}
pub async fn read_config(c: &Client) -> Result<Value> {
    let response = c.raw("GET", "/v1/config", None, HashMap::new()).await?;
    let mut d: Value = serde_json::from_str(&response.text).unwrap_or_else(|_| json!({}));
    if !(200..300).contains(&response.status) {
        return Err(Error::Game(crate::game::classify(
            "GET",
            "/v1/config",
            &response,
            d,
        )));
    }
    let revision = d["revision"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| crate::game::etag(&response.headers));
    if !d["text"].is_string() || revision.is_empty() || !d["sections"].is_array() {
        return Err(Error::Game(GameError {
            status: 502,
            code: "bad_response".into(),
            message: "The server did not return a config document.".into(),
            body: Value::Null,
            retry_after_ms: 0,
        }));
    }
    d["revision"] = json!(revision);
    Ok(
        json!({"revision":d["revision"],"writable":d["writable"]!=false,"text":d["text"],"sections":d["sections"],"warnings":default(&d,"warnings",json!([]))}),
    )
}
pub fn config_result(status: u16, body: Value, etag: &str) -> Value {
    let mut r = json!({"ok":(200..300).contains(&status)&&body["ok"]!=false,"status":status,"conflict":status==412,"revision":body["revision"].as_str().filter(|s|!s.is_empty()).unwrap_or(etag),"errorCode":body["error"]["code"].as_str().unwrap_or(""),"errorMessage":body["error"]["message"].as_str().map(str::to_owned).unwrap_or_else(||if (200..300).contains(&status){String::new()}else{format!("Request failed ({status}).")}),"timingsMs":body["timingsMs"]});
    for key in [
        "outcomes", "shadowed", "stripped", "errors", "changed", "warnings",
    ] {
        r[key] = default(&body, key, json!([]));
    }
    r["conflictDeltas"] = default(&body, "conflict", json!([]));
    r
}
async fn with_secrets(c: &Client, p: &Value) -> Result<String> {
    let text = p["text"].as_str().unwrap_or("");
    if !text.contains(ini::HIDDEN) {
        return Ok(text.into());
    }
    let live = read_config(c).await?;
    Ok(ini::restore(text, live["text"].as_str().unwrap_or(""))?)
}
fn no_route(e: &Error) -> bool {
    matches!(e,Error::Game(e) if e.code=="no_route"||e.status==405)
}
pub async fn edit_reserved(c: &Client, p: &Value, add: bool) -> Result<Value> {
    let id = steam(p)?;
    if !truthy(&p["viaConfig"]) {
        let r = if add {
            c.json("POST", "/v1/reserved-slots", Some(json!({"steamId":id})))
                .await
        } else {
            c.json("DELETE", &format!("/v1/reserved-slots/{id}"), None)
                .await
        };
        match r {
            Ok(v) => return Ok(v),
            Err(e) if no_route(&e) => (),
            Err(e) => return Err(e),
        }
    }
    for attempt in 0..2 {
        let doc = read_config(c).await?;
        let text = doc["text"].as_str().unwrap_or("");
        if doc["writable"] == false {
            return Err(Error::Game(GameError {
                status: 400,
                code: "config_readonly".into(),
                message:
                    "The reserved-slot routes are unavailable and the config document is read-only."
                        .into(),
                body: Value::Null,
                retry_after_ms: 0,
            }));
        }
        let mut ids = ini::reserved(text);
        let present = ids.contains(&id);
        if present == add {
            return Err(Error::Game(GameError {
                status: if add { 409 } else { 404 },
                code: if add {
                    "already_reserved"
                } else {
                    "reserved_not_found"
                }
                .into(),
                message: format!(
                    "SteamId {id} {}.",
                    if add {
                        "is already reserved"
                    } else {
                        "has no reserved slot"
                    }
                ),
                body: Value::Null,
                retry_after_ms: 0,
            }));
        }
        if add {
            ids.push(id.clone())
        } else {
            ids.retain(|s| s != &id)
        }
        let next = ini::set_array(text, ini::SESSION, ini::RESERVED, &ids);
        let (status, body, etag) = c
            .config_call("PUT", "/v1/config", next, doc["revision"].as_str())
            .await?;
        let result = config_result(status, body.clone(), &etag);
        if result["ok"] == true {
            let live = c.json("GET", "/v1/reserved-slots", None).await.ok();
            let pending = live
                .as_ref()
                .is_some_and(|v| array(&v["reservedSlots"]).iter().any(|v| v == &id) != add);
            let done = if add {
                format!("Reserved a slot for {id}")
            } else {
                format!("Removed the reserved slot for {id}")
            };
            return Ok(
                json!({"message":if pending{format!("{done} in the config document; the running list changes when the server restarts.")}else{format!("{done}.")},"via":"config","revision":result["revision"],"pendingRestart":pending}),
            );
        }
        if status == 412 {
            if attempt == 0 {
                continue;
            }
            return Err(Error::Game(GameError {
                status: 412,
                code: "revision_conflict".into(),
                message: "The config document changed twice; try again.".into(),
                body: Value::Null,
                retry_after_ms: 0,
            }));
        }
        let mut error = crate::game::classify(
            "PUT",
            "/v1/config",
            &crate::rcon::GameResponse {
                status,
                status_text: String::new(),
                headers: HashMap::new(),
                text: String::new(),
            },
            body,
        );
        error.message = ini::hide(json!(error.message), &[text])
            .as_str()
            .unwrap_or("Request failed.")
            .into();
        error.body = Value::Null;
        return Err(Error::Game(error));
    }
    unreachable!()
}
fn rotation_settings(p: &Value) -> Value {
    let mut out = json!({});
    if let Some(v) = p.get("rotationEnabled") {
        out["rotationEnabled"] = json!(v == true || v == "on" || v == "true")
    }
    if p.get("rotationMode").is_some() {
        out["rotationMode"] = json!(if s(p, "rotationMode", 100).to_lowercase() == "random" {
            "random"
        } else {
            "ordered"
        })
    }
    out
}
pub fn raw_path(p: &Value) -> Result<(String, String)> {
    let method = s(p, "method", 10).to_uppercase();
    if !["GET", "POST", "PUT", "PATCH", "DELETE"].contains(&method.as_str()) {
        return Err(ApiError::bad("Unsupported method.").into());
    }
    let path = crate::rcon::game_path(&s(p, "path", 500))
        .map_err(|_| ApiError::bad("Invalid raw path."))?;
    let route = path.split('?').next().unwrap_or("");
    if !route
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"/_~.-".contains(&b))
    {
        return Err(
            ApiError::bad("Raw routes must contain plain letters, digits and /_~.-.").into(),
        );
    }
    let lower = route.to_lowercase();
    for (prefix, code) in [
        ("/v1/config", "use_config_actions"),
        ("/v1/audit", "use_server_log"),
    ] {
        if lower.starts_with(prefix)
            && lower[prefix.len()..]
                .bytes()
                .next()
                .is_none_or(|b| !b.is_ascii_alphanumeric() && !b"_-".contains(&b))
        {
            return Err(ApiError::new(
                axum::http::StatusCode::FORBIDDEN,
                code,
                "Use the dedicated action to read this route.",
            )
            .into());
        }
    }
    Ok((method, path))
}
pub async fn run(c: &Client, name: &str, p: &Value) -> Result<Value> {
    match name {
        "health" => c.json("GET", "/v1/health", None).await,
        "capabilities" => Ok(capabilities_shape(
            c.json("GET", "/v1/capabilities", None).await?,
        )),
        "status" => Ok(status_shape(
            c.json("GET", "/v1/status", None).await?,
            truthy(&p["raw"]),
        )),
        "serverId" => {
            let d = c.json("GET", "/v1/server-id", None).await?;
            Ok(json!({"serverId":string(&d["serverId"],usize::MAX)}))
        }
        "players" => players_shape(&c.json("GET", "/v1/players", None).await?),
        "maps" | "lightings" => {
            let d = c.json("GET", &format!("/v1/catalog/{name}"), None).await?;
            Ok(
                json!({name:array(&d[name]).iter().map(|m|json!({"id":m["id"],"display":m.get("displayName").filter(|v|truthy(v)).unwrap_or(&m["id"])})).collect::<Vec<_>>()}),
            )
        }
        "experiences" => {
            let d = c.json("GET", "/v1/catalog/experiences", None).await?;
            let all=array(&d["experiences"]).iter().map(|m|json!({"id":m["id"],"display":m.get("displayName").filter(|v|truthy(v)).unwrap_or(&m["id"])})).collect::<Vec<_>>();
            if !truthy(&p["map"]) {
                Ok(json!({"experiences":all}))
            } else {
                let d = c
                    .json(
                        "GET",
                        &format!(
                            "/v1/catalog/maps/{}/experiences",
                            encoded(&s(p, "map", 100))
                        ),
                        None,
                    )
                    .await?;
                Ok(
                    json!({"experiences":array(&d["experiences"]).iter().map(|id|json!({"id":id,"display":all.iter().find(|m|&m["id"]==id).map(|m|&m["display"]).unwrap_or(id)})).collect::<Vec<_>>()}),
                )
            }
        }
        "alternators" => {
            let d = c
                .json(
                    "GET",
                    &format!(
                        "/v1/catalog/maps/{}/alternators",
                        encoded(&s(p, "map", 100))
                    ),
                    None,
                )
                .await?;
            Ok(
                json!({"alternators":array(&d["alternators"]).iter().map(|m|json!({"tag":m["tag"],"display":m.get("displayName").filter(|v|truthy(v)).unwrap_or(&m["tag"])})).collect::<Vec<_>>()}),
            )
        }
        "catalog" => {
            let mut result = json!({});
            for name in ["maps", "lightings", "experiences"] {
                let d = c.json("GET", &format!("/v1/catalog/{name}"), None).await?;
                result[name]=json!(array(&d[name]).iter().map(|m|json!({"id":m["id"],"display":m.get("displayName").filter(|v|truthy(v)).unwrap_or(&m["id"])})).collect::<Vec<_>>());
            }
            Ok(result)
        }
        "rotation" => Ok(rotation_shape(c.json("GET", "/v1/rotation", None).await?)),
        "bans" => {
            let d = c.json("GET", "/v1/bans", None).await?;
            Ok(
                json!({"bans":array(&d["bans"]).iter().map(|b|json!({"steamId":b["steamId"],"bannedAtUtc":if b["bannedAtUtc"].as_str().is_some_and(|s|s.starts_with("0001-")){json!("")}else{default(b,"bannedAtUtc",json!(""))},"bannedBy":default(b,"bannedBy",json!("")),"reason":default(b,"reason",json!(""))})).collect::<Vec<_>>()}),
            )
        }
        "reserved" => {
            let d = c.json("GET", "/v1/reserved-slots", None).await?;
            let mut r = json!({"reserved":default(&d,"reservedSlots",json!([]))});
            if truthy(&p["document"]) {
                r["document"] = match read_config(c).await {
                    Ok(d) => json!(ini::reserved(d["text"].as_str().unwrap_or(""))),
                    Err(Error::Game(_)) => Value::Null,
                    Err(e) => return Err(e),
                }
            }
            Ok(r)
        }
        "sponsor" => {
            let d = c.json("GET", "/v1/sponsor", None).await?;
            Ok(json!({"imageUrl":default(&d,"imageUrl",json!(""))}))
        }
        "serverLog" => {
            let limit = integer(&p["limit"], 50, 1, 500);
            let d = c
                .json("GET", &format!("/v1/audit?limit={limit}"), None)
                .await?;
            Ok(
                json!({"entries":array(&d["entries"]).iter().map(|e|json!({"timestampUtc":e["timestampUtc"],"peer":e["peer"],"sessionId":e["sessionId"],"event":e["event"],"detail":default(e,"detail",json!(""))})).collect::<Vec<_>>()}),
            )
        }
        "config" => {
            let mut doc = read_config(c).await?;
            let text = doc["text"].as_str().unwrap_or("").to_owned();
            doc["text"] = json!(ini::redact(&text));
            Ok(ini::hide(doc, &[&text]))
        }
        "broadcast" => {
            c.json(
                "POST",
                "/v1/broadcast",
                Some(json!({"message":required(p,"message",200)?})),
            )
            .await
        }
        "whisper" => {
            c.json(
                "POST",
                &format!("/v1/players/{}/message", steam(p)?),
                Some(json!({"message":required(p,"message",200)?})),
            )
            .await
        }
        "kick" => {
            let reason = s(p, "reason", 200);
            c.json(
                "POST",
                &format!("/v1/players/{}/kick", steam(p)?),
                Some(json!({"reason":if reason.is_empty(){"Kicked by admin.".into()}else{reason}})),
            )
            .await
        }
        "kill" => {
            c.json("POST", &format!("/v1/players/{}/kill", steam(p)?), None)
                .await
        }
        "changeTeam" => {
            let faction = required(p, "faction", 100)?;
            let id = steam(p)?;
            let mut moved = c
                .json(
                    "PATCH",
                    &format!("/v1/players/{id}"),
                    Some(json!({"faction":faction})),
                )
                .await?;
            let respawned = match c
                .json("POST", &format!("/v1/players/{id}/kill"), None)
                .await
            {
                Ok(_) => true,
                Err(Error::Game(_)) => false,
                Err(e) => return Err(e),
            };
            if !moved.is_object() {
                moved = json!({})
            }
            moved["respawned"] = json!(respawned);
            moved["message"] = json!(if respawned {
                format!("Moved to {faction} and killed, so they respawn on the new side.")
            } else {
                format!(
                    "Moved to {faction}. No living character to kill, so they spawn on the new side."
                )
            });
            Ok(moved)
        }
        "endMatch" | "restartMatch" => {
            c.json(
                "POST",
                if name == "endMatch" {
                    "/v1/match/end"
                } else {
                    "/v1/match/restart"
                },
                None,
            )
            .await
        }
        "changeMap" => {
            c.json("POST", "/v1/match/map", Some(map_selection(p)?))
                .await
        }
        "setWeather" => {
            c.json(
                "PUT",
                "/v1/world/lighting",
                Some(json!({"lighting":required(p,"lighting",100)?})),
            )
            .await
        }
        "setNextMap" => {
            let sel = map_selection(p)?;
            let rotation = rotation_shape(c.json("GET", "/v1/rotation", None).await?);
            let entries = array(&rotation["entries"]);
            let now = rotation["nowIndex"].as_i64().unwrap_or(-1);
            let mut index = entries
                .iter()
                .enumerate()
                .position(|(i, e)| i as i64 != now && same_selection(e, &sel))
                .map(|i| i as i64)
                .unwrap_or(-1);
            let mut now_after = now;
            let rest_last;
            if index < 0 {
                c.json("POST", "/v1/rotation/entries", Some(sel.clone()))
                    .await?;
                index = entries.len() as i64;
                rest_last = entries.len() as i64 - 1;
            } else {
                if now > index {
                    now_after = now - 1
                }
                rest_last = entries.len() as i64 - 2;
            }
            let slot = if now_after < 0 || now_after >= rest_last {
                0
            } else {
                now_after + 1
            };
            move_entry(c, index, slot).await?;
            Ok(
                json!({"message":format!("Next map set to {} (rotation entry {}).",sel["map"].as_str().unwrap_or(""),slot+1)}),
            )
        }
        "rotationAdd" => {
            c.json("POST", "/v1/rotation/entries", Some(map_selection(p)?))
                .await
        }
        "rotationRemove" => {
            c.json(
                "DELETE",
                &format!(
                    "/v1/rotation/entries/{}",
                    integer(&p["index"], -1, 0, 10000)
                ),
                None,
            )
            .await
        }
        "rotationMove" => {
            c.json(
                "POST",
                &format!(
                    "/v1/rotation/entries/{}/move",
                    integer(&p["index"], -1, 0, 10000)
                ),
                Some(json!({"direction":if p["direction"]=="up"{"up"}else{"down"}})),
            )
            .await
        }
        "rotationReorder" => {
            let from = integer(&p["from"], -1, 0, 10000);
            let to = integer(&p["to"], -1, 0, 10000);
            if from < 0 || to < 0 {
                return Err(ApiError::bad("from and to are required.").into());
            }
            move_entry(c, from, to).await?;
            Ok(json!({"message":format!("Moved rotation entry {} to position {}.",from+1,to+1)}))
        }
        "ban" => {
            let mut body = json!({"steamId":steam(p)?});
            let reason = s(p, "reason", 200);
            if !reason.is_empty() {
                body["reason"] = json!(reason)
            }
            c.json("POST", "/v1/bans", Some(body)).await
        }
        "unban" => {
            c.json("DELETE", &format!("/v1/bans/{}", steam(p)?), None)
                .await
        }
        "reservedAdd" | "reservedRemove" => edit_reserved(c, p, name == "reservedAdd").await,
        "rotationSave" => c.json("POST", "/v1/rotation/save", None).await,
        "rotationSettings" | "settings" => {
            let mut body = rotation_settings(p);
            if name == "settings" && p.get("scoreTick").is_some() {
                body["scoreTick"] = json!(integer(&p["scoreTick"], 24, 1, 600))
            }
            if body.as_object().unwrap().is_empty() {
                return Err(ApiError::bad("No settings to apply.").into());
            }
            c.json("PATCH", "/v1/settings", Some(body)).await
        }
        "configValidate" | "configApply" => {
            let text = with_secrets(c, p).await?;
            let mut path = if name == "configValidate" {
                "/v1/config/validate".to_owned()
            } else {
                "/v1/config".to_owned()
            };
            if name == "configApply" {
                let mut queries = vec![];
                if truthy(&p["force"]) {
                    queries.push("force=true")
                }
                if truthy(&p["fullApply"]) {
                    queries.push("fullApply=true")
                }
                if !queries.is_empty() {
                    path.push('?');
                    path.push_str(&queries.join("&"));
                }
            }
            let (status, body, etag) = c
                .config_call(
                    if name == "configValidate" {
                        "POST"
                    } else {
                        "PUT"
                    },
                    &path,
                    text.clone(),
                    if name == "configApply" {
                        p["revision"].as_str()
                    } else {
                        None
                    },
                )
                .await?;
            let result = config_result(status, body, &etag);
            let live = if result["conflict"] == true {
                read_config(c)
                    .await
                    .ok()
                    .and_then(|d| d["text"].as_str().map(str::to_owned))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            let result = ini::hide(result, &[&text, &live]);
            if name == "configApply" && result["ok"] != true && result["conflict"] != true {
                return Err(Error::Game(GameError {
                    status,
                    code: result["errorCode"].as_str().unwrap_or("").into(),
                    message: result["errorMessage"]
                        .as_str()
                        .unwrap_or("Config apply failed.")
                        .into(),
                    body: result,
                    retry_after_ms: 0,
                }));
            }
            Ok(result)
        }
        "raw" => {
            let (method, path) = raw_path(p)?;
            let mut headers = HashMap::new();
            let body = p.get("body").map(|b| {
                headers.insert(
                    "content-type".into(),
                    if b.is_string() {
                        "text/plain"
                    } else {
                        "application/json"
                    }
                    .into(),
                );
                if let Some(b) = b.as_str() {
                    b.into()
                } else {
                    b.to_string()
                }
            });
            let response = c.raw(&method, &path, body, headers).await?;
            let headers: HashMap<_, _> = response
                .headers
                .into_iter()
                .filter(|(k, _)| ["content-type", "etag", "retry-after"].contains(&k.as_str()))
                .collect();
            Ok(
                json!({"status":response.status,"headers":headers,"body":serde_json::from_str::<Value>(&response.text).unwrap_or_else(|_|json!(response.text))}),
            )
        }
        _ => Err(ApiError::missing().into()),
    }
}
async fn move_entry(c: &Client, from: i64, to: i64) -> Result<()> {
    let direction = if to < from { "up" } else { "down" };
    let mut current = from;
    while current != to {
        c.json(
            "POST",
            &format!("/v1/rotation/entries/{current}/move"),
            Some(json!({"direction":direction})),
        )
        .await?;
        current += if to < from { -1 } else { 1 };
    }
    Ok(())
}
pub fn fingerprint(text: &str) -> Value {
    use sha2::{Digest, Sha256};
    json!({"length":text.encode_utf16().count(),"sha256":hex::encode(Sha256::digest(text.as_bytes()))})
}
pub fn audit_detail(name: &str, p: &Value) -> Value {
    match name {
        "configValidate" => json!({"text":fingerprint(p["text"].as_str().unwrap_or(""))}),
        "configApply" => {
            json!({"text":fingerprint(p["text"].as_str().unwrap_or("")),"revision":s(p,"revision",100),"force":truthy(&p["force"]),"fullApply":truthy(&p["fullApply"])})
        }
        "raw" => {
            json!({"method":s(p,"method",10).to_uppercase(),"path":s(p,"path",500),"body":if let Some(t)=p["body"].as_str(){fingerprint(t)}else{p["body"].clone()}})
        }
        _ => p.clone(),
    }
}
pub fn target(name: &str, p: &Value) -> String {
    match name {
        "broadcast" => s(p, "message", 200),
        "whisper" | "kick" | "kill" | "changeTeam" | "ban" | "unban" | "reservedAdd"
        | "reservedRemove" => s(p, "steamId", 32),
        "changeMap" | "setNextMap" | "rotationAdd" => s(p, "map", 100),
        "setWeather" => s(p, "lighting", 100),
        "rotationRemove" => s(p, "index", 100),
        "rotationMove" => format!("{} {}", s(p, "index", 100), s(p, "direction", 20)),
        "rotationReorder" => format!("{} -> {}", s(p, "from", 100), s(p, "to", 100)),
        "configApply" => s(p, "revision", 100),
        "raw" => format!(
            "{} {}",
            s(p, "method", 10).to_uppercase(),
            s(p, "path", 300)
        ),
        _ => String::new(),
    }
}
