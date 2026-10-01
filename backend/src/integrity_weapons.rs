//! Tags describing a kill outrank an administrator's weapon classification.
use serde_json::{Value, json};
use std::collections::HashMap;
pub const CATEGORIES: &[&str] = &[
    "INFANTRY",
    "VEHICLE",
    "MORTAR",
    "ARTILLERY",
    "FIXED_AA",
    "CIWS",
    "FIXED_WEAPON",
    "ROADKILL",
    "ENVIRONMENT",
    "SUICIDE",
    "UNKNOWN",
];
pub const ARMS: &[&str] = &[
    "Id.Item.AK74M",
    "Id.Item.Mosin",
    "Id.Item.MP9",
    "Id.Item.WEPN_029",
    "Id.Item.M4",
    "Id.Item.M500",
    "Id.Item.MP43",
    "Id.Item.SKS",
    "Id.Item.SVDM",
    "Id.Item.KH2002",
    "Id.Item.TAR21",
    "Id.Item.A91",
    "Id.Item.SV98",
    "Id.Item.MK22",
    "Id.Item.Glock17",
    "Id.Item.CombatBow",
];
pub fn defaults() -> Value {
    Value::Object(
        ARMS.iter()
            .map(|c| ((*c).into(), json!("INFANTRY")))
            .collect(),
    )
}
pub fn classify<'a>(kill: &Value, overrides: &'a HashMap<String, String>) -> &'a str {
    let tagged = |tag: &str| {
        kill["tags"]
            .as_array()
            .is_some_and(|tags| tags.iter().any(|v| v == tag))
    };
    if kill["suicide"] == true || tagged("Suicide") {
        return "SUICIDE";
    }
    if tagged("Falling") {
        return "ENVIRONMENT";
    }
    if tagged("RoadKill") {
        return "ROADKILL";
    }
    if tagged("VehicleExplosion") {
        return "VEHICLE";
    }
    let Some(cause) = kill["cause"].as_str().filter(|s| !s.is_empty()) else {
        return "UNKNOWN";
    };
    if let Some(category) = overrides.get(cause) {
        return category;
    }
    if ARMS.contains(&cause) {
        return "INFANTRY";
    }
    let lower = cause.to_ascii_lowercase();
    if lower.starts_with("vehicle.") || lower.starts_with("id.vehicle.weaponextension.") {
        return "VEHICLE";
    }
    if lower.starts_with("id.buildable.") {
        return "FIXED_WEAPON";
    }
    "UNKNOWN"
}
pub fn infantry(kill: &Value, overrides: &HashMap<String, String>) -> bool {
    let nonempty = |key: &str| kill[key].as_str().is_some_and(|s| !s.is_empty());
    nonempty("killerSteamId")
        && kill["factionBracketed"] != false
        && (kill["factionBracketed"] != true || crate::http::truthy(&kill["factionObservedAt"]))
        && !crate::http::truthy(&kill["teamKill"])
        && kill["killerSteamId"] != kill["victimSteamId"]
        && nonempty("killerFaction")
        && nonempty("victimFaction")
        && kill["killerFaction"] != kill["victimFaction"]
        && classify(kill, overrides) == "INFANTRY"
}
pub async fn overrides(
    db: &sqlx::PgPool,
    org: &str,
) -> crate::error::Result<HashMap<String, String>> {
    Ok(
        sqlx::query_as("SELECT cause,category FROM integrity_weapon_map WHERE org_id=$1")
            .bind(org)
            .fetch_all(db)
            .await?
            .into_iter()
            .collect(),
    )
}
