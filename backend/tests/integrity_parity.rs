use serde_json::{Value, json};
use std::collections::HashMap;
use warcon_backend::{
    integrity_baseline, integrity_context as context, integrity_decisions as decisions,
    integrity_score as score, integrity_statistics as statistics, integrity_weapons as weapons,
    integrity_windows::{self, Windows},
};
#[test]
fn clean_reference_replay_weighting_and_numeric_rounds_match_original() {
    for (i, c) in fixture()["replays"].as_array().unwrap().iter().enumerate() {
        let mut replay = integrity_baseline::Replay::new(
            serde_json::from_value(c["overrides"].clone()).unwrap(),
        );
        for row in c["rows"].as_array().unwrap() {
            replay.add(&serde_json::from_value(row.clone()).unwrap());
        }
        let samples = replay.finish();
        close(
            &serde_json::to_value(&samples).unwrap(),
            &c["expected"],
            &format!("replay {i}"),
        );
        let cohorts = integrity_baseline::cohorts(&samples);
        close(
            &serde_json::to_value(&cohorts).unwrap(),
            &c["cohorts"],
            &format!("cohorts {i}"),
        );
        close(
            &serde_json::to_value(
                cohorts
                    .iter()
                    .map(integrity_baseline::summarize)
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
            &c["summaries"],
            &format!("summaries {i}"),
        );
    }
}
#[test]
fn independent_career_windows_match_original() {
    for (i, c) in fixture()["careers"].as_array().unwrap().iter().enumerate() {
        let rows = serde_json::from_value::<Vec<warcon_backend::integrity_career::History>>(
            c["rows"].clone(),
        )
        .unwrap();
        close(
            &warcon_backend::integrity_career::summarize(&rows),
            &c["expected"],
            &format!("career {i}"),
        );
    }
}
#[test]
fn empirical_statistics_and_context_selection_match_original() {
    let f = fixture();
    for (i, c) in f["statistics"].as_array().unwrap().iter().enumerate() {
        let baselines: HashMap<String, Value> =
            serde_json::from_value(c["baselines"].clone()).unwrap();
        let weapon_baselines: HashMap<String, Value> =
            serde_json::from_value(c["weaponBaselines"].clone()).unwrap();
        let actual = statistics::assess(
            &c["values"],
            &baselines,
            c["infantryKills"].as_u64().unwrap() as usize,
            c["independentEpisodes"].as_u64().unwrap() as usize,
            c["weaponObservations"].as_array().unwrap(),
            &weapon_baselines,
        )
        .unwrap();
        close(&actual, &c["expected"], &format!("statistics {i}"));
    }
    for (i, c) in f["selectors"].as_array().unwrap().iter().enumerate() {
        let now = chrono::DateTime::parse_from_rfc3339(c["now"].as_str().unwrap())
            .unwrap()
            .to_utc();
        for (weapon, key) in [(false, "expected"), (true, "weaponExpected")] {
            let actual = statistics::select(
                c["rows"].as_array().unwrap(),
                c["map"].as_str().unwrap(),
                c["bucket"].as_str(),
                now,
                Some(&c["state"]),
                c["server"].as_str(),
                weapon,
            );
            close(
                &serde_json::to_value(actual).unwrap(),
                &c[key],
                &format!("selector {i} {weapon}"),
            );
        }
    }
    for c in f["metadata"].as_array().unwrap() {
        assert_eq!(
            json!(statistics::population_bucket(c["count"].as_f64())),
            c["bucket"]
        );
        assert_eq!(
            statistics::sample_quality(c["count"].as_f64().unwrap_or(0.)),
            c["quality"]
        );
    }
}
#[test]
fn round_precision_change_minutes_and_independent_episodes_match_original() {
    let f = fixture();
    for (i, c) in f["precision"].as_array().unwrap().iter().enumerate() {
        let actual =
            context::compare_precision(c["rows"].as_array().unwrap(), c["id"].as_str().unwrap());
        close(&json!(actual), &c["expected"], &format!("precision {i}"));
        assert_eq!(
            json!(
                actual
                    .iter()
                    .map(context::precision_decision)
                    .collect::<Vec<_>>()
            ),
            c["decisions"]
        );
    }
    for (i, c) in f["series"].as_array().unwrap().iter().enumerate() {
        let clocks: Vec<f64> = serde_json::from_value(c["clocks"].clone()).unwrap();
        close(
            &json!(context::round_series(
                &clocks,
                c["from"].as_f64().unwrap(),
                c["clock"].as_f64().unwrap()
            )),
            &c["expected"],
            &format!("series {i}"),
        );
    }
    for (i, c) in f["minutes"].as_array().unwrap().iter().enumerate() {
        let clocks: Vec<f64> = serde_json::from_value(c["clocks"].clone()).unwrap();
        close(
            &context::consecutive_minutes(
                &clocks,
                c["clock"].as_f64().unwrap(),
                c["minutes"].as_u64().unwrap() as usize,
                c["threshold"].as_f64().unwrap(),
            ),
            &c["expected"],
            &format!("minutes {i}"),
        );
    }
    for (i, c) in f["episodes"].as_array().unwrap().iter().enumerate() {
        assert_eq!(
            context::independent_episode(
                &c["saved"],
                &c["current"],
                c["separation"].as_f64().unwrap()
            ),
            c["expected"].as_bool().unwrap(),
            "episode {i}"
        );
    }
}
#[test]
fn action_eligibility_and_independent_caps_match_original() {
    let f = fixture();
    for (i, c) in f["decisions"].as_array().unwrap().iter().enumerate() {
        assert_eq!(
            decisions::decide(&c["input"], c["statistical"].as_bool().unwrap()),
            c["expected"],
            "decision {i}"
        );
    }
    for (i, c) in f["caps"].as_array().unwrap().iter().enumerate() {
        assert_eq!(
            json!(decisions::cap(
                c["org"].as_f64().unwrap(),
                c["server"].as_f64().unwrap(),
                c["online"].as_f64().unwrap(),
                &c["settings"]
            )),
            c["expected"],
            "cap {i}"
        );
    }
    assert!(decisions::retry(
        &json!({"caseOpen":true,"hasAction":false,"lastAttemptAt":"2026-09-27T00:00:00Z","now":"2026-09-27T00:00:30Z"})
    ));
    assert!(!decisions::retry(
        &json!({"caseOpen":true,"hasAction":true,"lastAttemptAt":null})
    ));
    assert!(!statistics::action_eligible(
        &json!({"source":"local","code":"maxKillDistanceWeapon","sampleCount":1000,"uniquePlayers":100,"uniquePlayerDays":100,"effectiveSampleSize":900})
    ));
}
fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/integrity.json")).unwrap()
}
fn close(actual: &Value, expected: &Value, path: &str) {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => assert!(
            (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 1e-9,
            "{path}: {a} != {b}"
        ),
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{path}");
            for (i, (a, b)) in a.iter().zip(b).enumerate() {
                close(a, b, &format!("{path}[{i}]"));
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len(), "{path}: {a:?} != {b:?}");
            for (k, v) in b {
                close(&a[k], v, &format!("{path}.{k}"));
            }
        }
        _ => assert_eq!(actual, expected, "{path}"),
    }
}
#[test]
fn rule_validation_and_scores_match_original() {
    let f = fixture();
    assert_eq!(score::defaults(), f["defaults"]);
    for (i, c) in f["validators"].as_array().unwrap().iter().enumerate() {
        let result = score::validate(&c["patch"], &c["base"]);
        if let Some(message) = c["error"].as_str() {
            assert_eq!(result.unwrap_err().message, message, "validator {i}");
        } else {
            assert_eq!(result.unwrap(), c["expected"], "validator {i}");
        }
    }
    for (i, c) in f["scores"].as_array().unwrap().iter().enumerate() {
        let s: score::Signals = serde_json::from_value(c["signals"].clone()).unwrap();
        close(
            &serde_json::to_value(score::score(&s, &f["defaults"])).unwrap(),
            &c["expected"],
            &format!("score {i}"),
        );
    }
}
#[test]
fn weapon_classification_and_faction_gate_match_original() {
    for (i, c) in fixture()["weapons"].as_array().unwrap().iter().enumerate() {
        let overrides: HashMap<String, String> =
            serde_json::from_value(c["overrides"].clone()).unwrap();
        assert_eq!(
            weapons::classify(&c["kill"], &overrides),
            c["classification"],
            "classification {i}"
        );
        assert_eq!(
            weapons::infantry(&c["kill"], &overrides),
            c["infantry"],
            "infantry {i}"
        );
    }
}
#[test]
fn round_reset_delayed_events_deduplication_and_episodes_match_original() {
    let f = fixture();
    for (i, sequence) in f["windows"].as_array().unwrap().iter().enumerate() {
        let mut w = Windows::default();
        for (j, step) in sequence["steps"].as_array().unwrap().iter().enumerate() {
            let batch = step["batch"].as_array().unwrap();
            let output = integrity_windows::generate(
                &mut w,
                "server",
                batch,
                &HashMap::new(),
                &score::defaults(),
            );
            close(
                &serde_json::to_value(&output).unwrap(),
                &step["expected"],
                &format!("sequence {i} step {j}"),
            );
            close(
                &w.current("server", "killer").unwrap_or(Value::Null),
                &step["current"],
                "current",
            );
            close(
                &serde_json::to_value(w.snapshots(
                    "server",
                    &["killer".into(), "other".into(), "killer".into()],
                ))
                .unwrap(),
                &step["snapshots"],
                "snapshots",
            );
            for f in &output.findings {
                w.mark_persisted("server", f, 101 + j as i64);
            }
        }
    }
}
#[test]
fn extreme_distance_remains_invalid_and_not_in_weapon_metrics() {
    let invalid = json!({"cause":"Id.Item.AK74M","distanceM":5000});
    let entry = integrity_windows::Entry {
        clock: 1.,
        event_id: "e".into(),
        victim: "v".into(),
        cause: invalid["cause"].as_str().map(str::to_owned),
        distance: invalid["distanceM"].as_f64(),
        headshot: false,
        penetration: false,
    };
    assert!(integrity_windows::weapon_metrics(&[&entry])[0]["maxKillDistanceM"].is_null());
}
