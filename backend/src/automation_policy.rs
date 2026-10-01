use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NumericLimits {
    pub enabled: bool,
    pub kpm: Option<f64>,
    pub kd: Option<f64>,
    pub cash: Option<f64>,
    pub window_seconds: i64,
    pub min_kills: i64,
}
impl NumericLimits {
    pub fn parse(v: &Value) -> Option<Self> {
        for key in ["kpm", "kd", "cash"] {
            if !v.get(key).is_some_and(|v| {
                v.is_null()
                    || v.as_f64()
                        .is_some_and(|n| n.is_finite() && n > 0. && n <= 100000000.)
            }) {
                return None;
            }
        }
        if !integer_range(&v["windowSeconds"], 60, 900) || !integer_range(&v["minKills"], 1, 1000) {
            return None;
        }
        let p = Self {
            enabled: v["enabled"].as_bool()?,
            kpm: v["kpm"].as_f64(),
            kd: v["kd"].as_f64(),
            cash: v["cash"].as_f64(),
            window_seconds: v["windowSeconds"].as_f64()? as i64,
            min_kills: v["minKills"].as_f64()? as i64,
        };
        ((60..=900).contains(&p.window_seconds)
            && (1..=1000).contains(&p.min_kills)
            && (!p.enabled || p.kpm.is_some() || p.kd.is_some() || p.cash.is_some()))
        .then_some(p)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LimitObservation {
    pub at: f64,
    pub kills: f64,
    pub deaths: f64,
    pub cash: f64,
}
#[derive(Debug, Serialize, PartialEq)]
pub struct Breach {
    pub metric: &'static str,
    pub value: f64,
    pub limit: f64,
}
pub fn numeric_breaches(rule: &NumericLimits, points: &[LimitObservation]) -> Vec<Breach> {
    if points.len() < 2 {
        return vec![];
    }
    let first = &points[0];
    let last = &points[points.len() - 1];
    let seconds = (last.at - first.at) / 1000.;
    if seconds < rule.window_seconds as f64 || seconds > (rule.window_seconds + 60) as f64 {
        return vec![];
    }
    if points.iter().any(|p| {
        ![p.at, p.kills, p.deaths, p.cash]
            .iter()
            .all(|n| n.is_finite())
            || p.kills < 0.
            || p.deaths < 0.
    }) || points.windows(2).any(|p| {
        p[1].at <= p[0].at
            || p[1].at - p[0].at > 65000.
            || p[1].kills < p[0].kills
            || p[1].deaths < p[0].deaths
    }) {
        return vec![];
    }
    [
        ("kpm", rule.kpm, (last.kills - first.kills) * 60. / seconds),
        ("kd", rule.kd, last.kills / last.deaths.max(1.)),
        ("cash", rule.cash, (last.cash - first.cash) * 60. / seconds),
    ]
    .into_iter()
    .filter_map(|(metric, limit, value)| {
        limit
            .filter(|limit| {
                value > *limit && (metric != "kd" || last.kills >= rule.min_kills as f64)
            })
            .map(|limit| Breach {
                metric,
                limit,
                value,
            })
    })
    .collect()
}
pub fn integer_range(v: &Value, min: i64, max: i64) -> bool {
    v.as_f64()
        .is_some_and(|n| n.is_finite() && n.fract() == 0. && n >= min as f64 && n <= max as f64)
}
