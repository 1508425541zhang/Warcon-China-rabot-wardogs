//! Original explainable score and validation. Statistical voting is a separate engine.
use crate::error::{ApiError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub fn defaults() -> Value {
    json!({"committeeKpmMinutes":3,"kpmBands":[{"min":4,"points":18},{"min":4.5,"points":24},{"min":5,"points":32},{"min":6,"points":42},{"min":8,"points":52}],"uniqueVictimBands":[{"min":8,"points":3},{"min":12,"points":6},{"min":18,"points":10}],"reportBands":[{"min":1,"points":1},{"min":3,"points":4},{"min":5,"points":8},{"min":8,"points":12},{"min":12,"points":16}],"repeatWindowMinutes":15,"repeatSecond":8,"repeatThird":14,"repeatBoth5":8,"repeatBoth6":14,"repeatKo":10,"repeatKoWindowHours":24,"steamPriorCap":15,"recentVac":8,"recentGameBan":12,"oldVac":1,"oldGameBan":2,"lowPlaytimeHours":2,"lowPlaytimeKpm":6,"lowPlaytimePoints":5,"headshotMinKills":15,"headshotMinPct":70,"headshotMax":10,"penetrationMinKills":15,"penetrationMinPct":50,"penetrationMax":6,"burstMax":20,"burstFindingMin":7,"passiveWatchThreshold":20,"activeWatchThreshold":40,"koThreshold":54,"quarantineThreshold":64,"quarantineDays":365,"minimumOnlineForAutoAction":20,"mode":"dry_run"})
}
pub fn validate(patch: &Value, base: &Value) -> Result<Value> {
    let patch = patch
        .as_object()
        .ok_or_else(|| ApiError::bad("Integrity rules must be an object."))?;
    let mut next = defaults();
    let allowed = next.as_object().unwrap().clone();
    for key in patch.keys() {
        if !allowed.contains_key(key) {
            return Err(ApiError::bad(format!("Unknown integrity rule '{key}'.")));
        }
    }
    if let Some(base) = base.as_object() {
        next.as_object_mut().unwrap().extend(base.clone());
    }
    next.as_object_mut().unwrap().extend(patch.clone());
    for (key, min, max) in [
        ("committeeKpmMinutes", 2., 3.),
        ("repeatWindowMinutes", 1., 120.),
        ("repeatSecond", 0., 30.),
        ("repeatThird", 0., 30.),
        ("repeatBoth5", 0., 30.),
        ("repeatBoth6", 0., 30.),
        ("repeatKo", 0., 30.),
        ("repeatKoWindowHours", 1., 720.),
        ("steamPriorCap", 0., 30.),
        ("recentVac", 0., 30.),
        ("recentGameBan", 0., 30.),
        ("oldVac", 0., 30.),
        ("oldGameBan", 0., 30.),
        ("lowPlaytimeHours", 0., 100.),
        ("lowPlaytimeKpm", 1., 20.),
        ("lowPlaytimePoints", 0., 15.),
        ("headshotMinKills", 1., 100.),
        ("headshotMinPct", 1., 100.),
        ("headshotMax", 0., 10.),
        ("penetrationMinKills", 1., 100.),
        ("penetrationMinPct", 1., 100.),
        ("penetrationMax", 0., 6.),
        ("burstMax", 1., 20.),
        ("burstFindingMin", 1., 12.),
        ("passiveWatchThreshold", 1., 99.),
        ("activeWatchThreshold", 1., 99.),
        ("koThreshold", 1., 100.),
        ("quarantineThreshold", 1., 100.),
        ("quarantineDays", 1., 3650.),
        ("minimumOnlineForAutoAction", 1., 100.),
    ] {
        if next[key]
            .as_f64()
            .is_none_or(|n| !n.is_finite() || n < min || n > max)
        {
            return Err(ApiError::bad(format!(
                "{key} must be between {min} and {max}."
            )));
        }
    }
    for key in [
        "committeeKpmMinutes",
        "burstFindingMin",
        "repeatKoWindowHours",
        "minimumOnlineForAutoAction",
    ] {
        if num(&next, key).fract() != 0. {
            return Err(ApiError::bad(
                "Burst and repeat KO periods must be whole numbers.",
            ));
        }
    }
    for key in ["kpmBands", "uniqueVictimBands", "reportBands"] {
        let bands = next[key].as_array().ok_or_else(|| {
            ApiError::bad(format!(
                "{key} must contain ascending thresholds and weights."
            ))
        })?;
        let valid = !bands.is_empty()
            && bands.len() <= 10
            && bands.iter().enumerate().all(|(i, b)| {
                let min = b["min"].as_f64();
                let points = b["points"].as_f64();
                min.is_some_and(|m| m.is_finite() && m >= 1.)
                    && points.is_some_and(|p| {
                        p.is_finite() && p.fract() == 0. && (0. ..=100.).contains(&p)
                    })
                    && (i == 0
                        || (min.unwrap() > num(&bands[i - 1], "min")
                            && points.unwrap() >= num(&bands[i - 1], "points")))
            });
        if !valid {
            return Err(ApiError::bad(format!(
                "{key} must contain ascending thresholds and weights."
            )));
        }
    }
    if next["mode"] != "dry_run" {
        return Err(ApiError::bad(
            "Integrity enforcement is not available in this phase.",
        ));
    }
    if !(num(&next, "passiveWatchThreshold") < num(&next, "activeWatchThreshold")
        && num(&next, "activeWatchThreshold") < num(&next, "koThreshold")
        && num(&next, "koThreshold") < num(&next, "quarantineThreshold"))
    {
        return Err(ApiError::bad(
            "Risk thresholds must increase from watch to quarantine.",
        ));
    }
    if num(&next, "oldVac") > num(&next, "recentVac")
        || num(&next, "oldGameBan") > num(&next, "recentGameBan")
    {
        return Err(ApiError::bad(
            "Old Steam ban weights cannot exceed recent weights.",
        ));
    }
    Ok(next)
}
pub fn num(value: &Value, key: &str) -> f64 {
    value[key].as_f64().unwrap_or(0.)
}
pub fn tier(value: f64, bands: &Value) -> f64 {
    bands
        .as_array()
        .into_iter()
        .flatten()
        .filter(|b| value >= num(b, "min"))
        .map(|b| num(b, "points"))
        .fold(0., f64::max)
}
pub fn level(score: f64, config: &Value) -> &'static str {
    if score >= num(config, "quarantineThreshold") {
        "AUTO_QUARANTINE_ELIGIBLE"
    } else if score >= num(config, "koThreshold") {
        "AUTO_KO"
    } else if score >= num(config, "activeWatchThreshold") {
        "ACTIVE_WATCH"
    } else if score >= num(config, "passiveWatchThreshold") {
        "PASSIVE_WATCH"
    } else {
        "NORMAL"
    }
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Signals {
    #[serde(default)]
    pub committee_mode: bool,
    pub behavior_reasons: Vec<String>,
    pub kpm180: f64,
    pub unique_victims: f64,
    pub previous_kpm: Vec<f64>,
    pub unique_reporters: f64,
    pub repeat_high_risk_window: bool,
    pub infantry_kills: f64,
    pub headshots: f64,
    pub penetrations: f64,
    pub burst_points: f64,
    pub vac_bans: f64,
    pub game_bans: f64,
    pub days_since_last_ban: Option<f64>,
    pub wardogs_playtime_hours: Option<f64>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Score {
    pub score: f64,
    pub level: &'static str,
    pub breakdown: Vec<Value>,
    pub current_behavior_anomaly: bool,
}
pub fn score(s: &Signals, c: &Value) -> Score {
    let mut breakdown = Vec::new();
    let mut add = |code: &str, points: f64, detail: String| {
        if points > 0. {
            breakdown.push(json!({"code":code,"points":points,"detail":detail}));
        }
    };
    let kpm_points = if s.committee_mode {
        if s.kpm180 > 2. { 6. } else { 0. }
    } else {
        tier(s.kpm180, &c["kpmBands"])
    };
    add(
        if s.committee_mode {
            "committee_kpm_watch"
        } else {
            "infantry_kpm_180"
        },
        kpm_points,
        format!("{:.2} infantry KPM", s.kpm180),
    );
    if kpm_points > 0. && !s.committee_mode {
        add(
            "unique_victims",
            tier(s.unique_victims, &c["uniqueVictimBands"]),
            format!("{} unique victims", s.unique_victims),
        );
        let prior: Vec<_> = s
            .previous_kpm
            .iter()
            .copied()
            .filter(|n| *n >= num(&c["kpmBands"][0], "min"))
            .collect();
        add(
            "repeat_window",
            if prior.len() >= 2 {
                num(c, "repeatThird")
            } else if prior.len() == 1 {
                num(c, "repeatSecond")
            } else {
                0.
            },
            format!("{} independent abnormal windows", prior.len() + 1),
        );
        if prior.iter().any(|n| *n >= 6.) && s.kpm180 >= 6. {
            add(
                "repeat_extreme",
                num(c, "repeatBoth6"),
                "Two windows at 6+ KPM".into(),
            );
        } else if prior.iter().any(|n| *n >= 5.) && s.kpm180 >= 5. {
            add(
                "repeat_extreme",
                num(c, "repeatBoth5"),
                "Two windows at 5+ KPM".into(),
            );
        }
    }
    add(
        "unique_reports",
        tier(s.unique_reporters, &c["reportBands"]),
        format!("{} unique reporters", s.unique_reporters),
    );
    add(
        "repeat_high_risk_window",
        if s.repeat_high_risk_window {
            num(c, "repeatKo")
        } else {
            0.
        },
        "Another high-risk window in the review period".into(),
    );
    if s.infantry_kills >= num(c, "headshotMinKills") {
        add(
            "headshots",
            if s.headshots / s.infantry_kills >= num(c, "headshotMinPct") / 100. {
                num(c, "headshotMax")
            } else {
                0.
            },
            format!("{}/{} infantry headshots", s.headshots, s.infantry_kills),
        );
    }
    if s.infantry_kills >= num(c, "penetrationMinKills") {
        add(
            "penetrations",
            if s.penetrations / s.infantry_kills >= num(c, "penetrationMinPct") / 100. {
                num(c, "penetrationMax")
            } else {
                0.
            },
            format!(
                "{}/{} infantry penetrations",
                s.penetrations, s.infantry_kills
            ),
        );
    }
    add(
        "kill_burst",
        num(c, "burstMax").min(s.burst_points.max(0.)),
        "Server-feed kill burst".into(),
    );
    let recent = s.days_since_last_ban.is_some_and(|d| d <= 365.);
    let prior = num(c, "steamPriorCap").min(
        s.vac_bans.max(0.) * num(c, if recent { "recentVac" } else { "oldVac" })
            + s.game_bans.max(0.)
                * num(
                    c,
                    if recent {
                        "recentGameBan"
                    } else {
                        "oldGameBan"
                    },
                ),
    );
    add(
        "steam_ban_prior",
        prior,
        "Public Steam ban history, time-decayed".into(),
    );
    if s.wardogs_playtime_hours
        .is_some_and(|h| h < num(c, "lowPlaytimeHours"))
        && s.kpm180 >= num(c, "lowPlaytimeKpm")
    {
        add(
            "low_playtime",
            num(c, "lowPlaytimePoints"),
            "Low public WARDOGS playtime with extreme KPM".into(),
        );
    }
    let score = breakdown
        .iter()
        .map(|b| num(b, "points"))
        .sum::<f64>()
        .min(100.);
    Score {
        score,
        level: level(score, c),
        breakdown,
        current_behavior_anomaly: !s.behavior_reasons.is_empty()
            || (s.committee_mode && s.kpm180 > 2.),
    }
}
