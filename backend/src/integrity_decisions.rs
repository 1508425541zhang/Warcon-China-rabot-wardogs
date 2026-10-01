//! Recommendation gates, frozen-case reuse and delivery accounting. No game requests here.
use crate::{
    committee::{self, Verdict},
    http::truthy,
    integrity_score::num,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashSet;
pub fn decide(input: &Value, statistical: bool) -> &'static str {
    let settings = &input["settings"];
    let finding = &input["finding"];
    let rules = &input["rules"];
    if truthy(&settings["autoSuspendedAt"])
        || !truthy(&input["feedHealthy"])
        || !truthy(&input["playerOnline"])
        || num(input, "onlinePlayers") < num(rules, "minimumOnlineForAutoAction")
        || !truthy(&input["identityReliable"])
        || !["A", "B"].contains(&input["confidence"].as_str().unwrap_or(""))
    {
        return "OBSERVE";
    }
    if statistical {
        let a = &input["assessment"];
        if !truthy(&settings["autoKickEnabled"])
            || a["status"] != "READY"
            || a["committee"]["decision"] != "KICK_CANDIDATE"
            || truthy(&a["committee"]["autoActionBlocked"])
            || a["level"] != "KICK_CANDIDATE"
        {
            return "OBSERVE";
        }
        let votes: Vec<Verdict> =
            serde_json::from_value(a["committee"]["verdicts"].clone()).unwrap_or_default();
        return if committee::direct_kick(&votes, num(finding, "kpm180")) {
            "KICK"
        } else {
            "OBSERVE"
        };
    }
    let score = &input["score"];
    if !truthy(&score["currentBehaviorAnomaly"]) {
        return "OBSERVE";
    }
    let reasons: HashSet<_> = finding["reasons"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let extreme = num(finding, "kpm180") >= 8. || num(finding, "burstPoints") >= 12.;
    let strong = reasons.len() >= 2 || (extreme && truthy(&input["priorIndependentWindow"]));
    let ko = reasons.len() >= 2 || extreme;
    if !ko || num(score, "score") < num(rules, "koThreshold") {
        return "OBSERVE";
    }
    if strong && num(score, "score") >= num(rules, "quarantineThreshold") {
        let repeat = input["previousActions"].as_array().is_some_and(|a| {
            a.iter().any(|v| {
                matches!(
                    v.as_str(),
                    Some("KICK" | "QUARANTINE_24H" | "QUARANTINE_7D")
                )
            })
        });
        let exceptional = num(finding, "kpm180") >= 8.
            && num(finding, "burstPoints") >= 12.
            && reasons.len() >= 3;
        if truthy(&settings["autoQuarantine7dEnabled"]) && (repeat || exceptional) {
            return "QUARANTINE_7D";
        }
        if truthy(&settings["autoQuarantine24hEnabled"]) {
            return "QUARANTINE_24H";
        }
    }
    if truthy(&settings["autoKickEnabled"]) {
        "KICK"
    } else {
        "OBSERVE"
    }
}
pub fn cap(org: f64, server: f64, online: f64, settings: &Value) -> Option<&'static str> {
    if org >= num(settings, "autoActionMaxPerHour") {
        Some("org_hourly")
    } else if server
        >= (online * num(settings, "autoActionMaxPercentOnline") / 100.)
            .floor()
            .max(1.)
    {
        Some("server_percent")
    } else {
        None
    }
}
pub fn action_version(assessment: &Value, state: &Value) -> bool {
    assessment.is_object()
        && state.is_object()
        && state["baselineStatus"] == "READY"
        && assessment["modelVersion"] == committee::MODEL_VERSION
        && assessment["featureVersion"] == committee::FEATURE_VERSION
        && assessment["weaponMapVersion"] == state["weaponMapVersion"]
        && truthy(&assessment["baselineGeneration"])
        && assessment["baselineGeneration"] == state["activeBaselineGeneration"]
        && assessment["committee"]["generation"] == committee::MODEL_VERSION
        && assessment["committee"]["decision"] == "KICK_CANDIDATE"
        && !truthy(&assessment["committee"]["autoActionBlocked"])
}
pub fn retry(input: &Value) -> bool {
    let time = |key: &str| {
        input[key]
            .as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
    };
    input["caseOpen"] == true
        && !truthy(&input["hasAction"])
        && (input["lastAttemptAt"].is_null()
            || time("lastAttemptAt")
                .zip(time("now"))
                .is_some_and(|(a, b)| (b - a).num_milliseconds() >= 30000))
}
pub fn reuse(saved: &Value, current: &Value) -> bool {
    saved.is_object()
        && ["policyVersion", "passed", "requiredMinutes", "threshold"]
            .iter()
            .all(|k| saved["sustainedKpm"][*k] == current["sustainedKpm"][*k])
        && [
            "modelVersion",
            "featureVersion",
            "weaponMapVersion",
            "baselineGeneration",
        ]
        .iter()
        .all(|k| saved[*k] == current[*k])
        && !(truthy(&saved["committee"]["autoActionBlocked"])
            && current["committee"]["autoActionBlocked"] == false)
}
pub fn confidence(expected: &[String], found: &[String]) -> &'static str {
    if expected.is_empty() || found.is_empty() {
        "D"
    } else if expected.len() == found.len()
        && expected.len() == found.iter().collect::<HashSet<_>>().len()
        && expected.iter().all(|e| found.contains(e))
    {
        "B"
    } else {
        "C"
    }
}
pub fn effective_actions(rows: &[Value], now: DateTime<Utc>) -> Vec<String> {
    rows.iter()
        .filter(|r| {
            r["source"] == "RULE"
                && (r["action"] != "KICK" || r["deliveryState"] == "delivered")
                && !r["effectiveAt"].is_null()
                && r["revertedAt"].is_null()
                && (r["action"] == "KICK"
                    || r["expiresAt"]
                        .as_str()
                        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                        .is_some_and(|t| t > now))
                && matches!(
                    r["action"].as_str(),
                    Some("KICK" | "QUARANTINE_24H" | "QUARANTINE_7D")
                )
        })
        .filter_map(|r| r["action"].as_str().map(str::to_owned))
        .collect()
}
