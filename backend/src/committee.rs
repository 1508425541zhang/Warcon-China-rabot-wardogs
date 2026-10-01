//! Five-expert mathematics. This module makes recommendations; delivery protections live elsewhere.
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub const MODEL_VERSION: &str = "ensemble-server-round-v3";
pub const FEATURE_VERSION: &str = "rolling-infantry-v2";
pub const VOTING_VERSION: &str = "five-vote-3s-2c-review-3c-kick";
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Decision {
    Unknown,
    Normal,
    Suspicious,
    CheatLikely,
}
impl Decision {
    fn rank(self) -> u8 {
        match self {
            Self::Unknown => 0,
            Self::Normal => 1,
            Self::Suspicious => 2,
            Self::CheatLikely => 3,
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Verdict {
    pub model_id: String,
    pub model_version: String,
    pub evidence_family: String,
    pub decision: Decision,
    pub confidence: f64,
    pub evidence_quality: f64,
    pub reasons: Vec<String>,
    pub evidence_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hard_evidence: Option<bool>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Committee {
    pub voting_version: String,
    pub generation: String,
    pub verdicts: Vec<Verdict>,
    pub cheat_votes: usize,
    pub suspicious_votes: usize,
    pub normal_votes: usize,
    pub unknown_votes: usize,
    pub participating_models: usize,
    pub independent_cheat_families: usize,
    pub independent_suspicious_families: usize,
    pub decision: String,
    pub auto_action_blocked: bool,
    pub veto_reasons: Vec<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input {
    pub statistical: Value,
    #[serde(default)]
    pub precision: Vec<Precision>,
    pub independent_episodes: u64,
    #[serde(default)]
    pub career: Option<Career>,
    #[serde(default)]
    pub change_series: Option<Vec<f64>>,
    pub current_kpm: f64,
    pub event_ids: Vec<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Career {
    pub sample_count: u64,
    pub unique_days: u64,
    pub kpm_median: f64,
    pub kpm_mad: Option<f64>,
    pub recent_kpm: Vec<f64>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Precision {
    pub category: String,
    pub kills: u64,
    pub headshots: u64,
    pub rate: f64,
    pub peer_players: u64,
    pub peer_kills: u64,
    pub peer_headshots: u64,
    pub server_rate: Option<f64>,
    pub difference: Option<f64>,
    pub ratio: Option<f64>,
    pub percentile: Option<f64>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Quality {
    pub feed_healthy: bool,
    pub backlog_safe: bool,
    pub identity_reliable: bool,
    pub round_reliable: bool,
    pub baseline_fresh: bool,
    pub versions_match: bool,
    pub baseline_population_adequate: bool,
}
impl Quality {
    pub fn veto(&self) -> Vec<String> {
        [
            ("FEEDHEALTHY", self.feed_healthy),
            ("BACKLOGSAFE", self.backlog_safe),
            ("IDENTITYRELIABLE", self.identity_reliable),
            ("ROUNDRELIABLE", self.round_reliable),
            ("BASELINEFRESH", self.baseline_fresh),
            ("VERSIONSMATCH", self.versions_match),
            (
                "BASELINEPOPULATIONADEQUATE",
                self.baseline_population_adequate,
            ),
        ]
        .into_iter()
        .filter(|(_, ok)| !*ok)
        .map(|(key, _)| key.into())
        .collect()
    }
}
pub fn vote(verdicts: Vec<Verdict>, veto_reasons: Vec<String>) -> Committee {
    let mut votes: Vec<&Verdict> = Vec::new();
    for item in &verdicts {
        if let Some(current) = votes.iter_mut().find(|v| v.model_id == item.model_id) {
            if item.decision.rank() > current.decision.rank() {
                *current = item
            }
        } else {
            votes.push(item)
        }
    }
    let count = |decision| votes.iter().filter(|v| v.decision == decision).count();
    let cheat = count(Decision::CheatLikely);
    let suspicious = count(Decision::Suspicious);
    let normal = count(Decision::Normal);
    let unknown = count(Decision::Unknown);
    let participating = votes.len() - unknown;
    let decision = if cheat >= 3 {
        "KICK_CANDIDATE"
    } else if cheat >= 2 || cheat + suspicious >= 3 {
        "CASE"
    } else if cheat + suspicious >= 2 {
        "WATCH"
    } else {
        "NORMAL"
    }
    .to_owned();
    Committee {
        voting_version: VOTING_VERSION.into(),
        generation: MODEL_VERSION.into(),
        verdicts,
        cheat_votes: cheat,
        suspicious_votes: suspicious,
        normal_votes: normal,
        unknown_votes: unknown,
        participating_models: participating,
        independent_cheat_families: cheat,
        independent_suspicious_families: suspicious,
        decision,
        auto_action_blocked: !veto_reasons.is_empty(),
        veto_reasons,
    }
}
pub fn direct_kick(verdicts: &[Verdict], kpm: f64) -> bool {
    let mut high = std::collections::HashSet::new();
    for v in verdicts {
        if v.decision == Decision::CheatLikely {
            high.insert(&v.model_id);
        }
    }
    high.len() >= 3
        || (kpm.is_finite()
            && kpm > 4.
            && verdicts.iter().any(|v| {
                v.model_id != "tempo"
                    && matches!(v.decision, Decision::Suspicious | Decision::CheatLikely)
            }))
}
fn threshold(category: &str) -> Option<(u64, u64, &str)> {
    match category {
        "automatic" => Some((60, 70, "步枪／机枪／冲锋枪")),
        "sniper" => Some((70, 90, "狙击枪")),
        "shotgun" => Some((30, 50, "霰弹枪")),
        _ => None,
    }
}
pub fn precision_decision(row: &Precision) -> Decision {
    let Some((watch, high, _)) = threshold(&row.category) else {
        return Decision::Unknown;
    };
    if row.kills < 5 {
        return Decision::Unknown;
    }
    let tail = |p| row.percentile.is_some_and(|v| v >= p) && row.difference.is_some_and(|v| v > 0.);
    if (row.headshots as u128) * 100 >= (high as u128) * (row.kills as u128) || tail(0.95) {
        Decision::CheatLikely
    } else if (row.headshots as u128) * 100 >= (watch as u128) * (row.kills as u128) || tail(0.9) {
        Decision::Suspicious
    } else {
        Decision::Normal
    }
}
fn verdict(
    input: &Input,
    id: &str,
    family: &str,
    decision: Decision,
    reasons: Vec<String>,
) -> Verdict {
    let quality = if decision == Decision::Unknown {
        0.
    } else {
        1.
    };
    Verdict {
        model_id: id.into(),
        model_version: if id == "precision" {
            "2-round-class"
        } else {
            "1"
        }
        .into(),
        evidence_family: family.into(),
        decision,
        confidence: quality,
        evidence_quality: quality,
        reasons,
        evidence_refs: input.event_ids.clone(),
        hard_evidence: None,
    }
}
pub fn expert_verdicts(input: &Input) -> Vec<Verdict> {
    let strongest = input.statistical["metrics"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| {
            [
                "kpm180",
                "uniqueVictims",
                "maxKills15s",
                "medianKillInterval",
            ]
            .contains(&m["code"].as_str().unwrap_or(""))
                && m["source"] == "local"
                && m["sampleCount"].as_f64().unwrap_or(0.) >= 50.
        })
        .map(|m| {
            (
                m,
                m["extremenessPercentile"].as_f64().unwrap_or(0.).min(
                    m["sampleCount"].as_f64().unwrap_or(0.)
                        / (m["sampleCount"].as_f64().unwrap_or(0.) + 1.),
                ),
            )
        })
        .fold(None, |current: Option<(&Value, f64)>, item| {
            if current.is_none_or(|c| item.1 > c.1) {
                Some(item)
            } else {
                current
            }
        });
    let tempo = if let Some((m, p)) = strongest {
        verdict(
            input,
            "tempo",
            "TEMPO",
            if p >= 0.95 {
                Decision::CheatLikely
            } else if p >= 0.9 {
                Decision::Suspicious
            } else {
                Decision::Normal
            },
            vec![m["code"].as_str().unwrap_or("").into()],
        )
    } else {
        verdict(
            input,
            "tempo",
            "TEMPO",
            Decision::Unknown,
            vec!["NO_CLEAN_TEMPO_BASELINE".into()],
        )
    };
    let chosen = input
        .precision
        .iter()
        .map(|row| (row, precision_decision(row)))
        .fold(None, |current: Option<(&Precision, Decision)>, item| {
            if current.is_none_or(|c| item.1.rank() > c.1.rank()) {
                Some(item)
            } else {
                current
            }
        });
    let precision = if let Some((r, decision)) = chosen.filter(|(_, d)| *d != Decision::Unknown) {
        let (_, _, label) = threshold(&r.category).unwrap();
        let comparison = if let Some(rate) = r.server_rate {
            format!(
                "本服同类武器其他玩家平均 {:.1}%，差值 {:.1} 个百分点，分位 {:.1}%，参考 {} 人／{} 杀",
                rate * 100.,
                r.difference.unwrap_or(0.) * 100.,
                r.percentile.unwrap_or(0.) * 100.,
                r.peer_players,
                r.peer_kills
            )
        } else {
            "本服同类武器暂无其他玩家参考，按分类阈值判断".into()
        };
        verdict(
            input,
            "precision",
            "PRECISION",
            decision,
            vec![
                format!(
                    "{label}：本局 {} 杀／{} 次爆头，爆头率 {:.1}%",
                    r.kills,
                    r.headshots,
                    r.rate * 100.
                ),
                comparison,
            ],
        )
    } else {
        verdict(
            input,
            "precision",
            "PRECISION",
            Decision::Unknown,
            vec!["ROUND_PRECISION_FEWER_THAN_FIVE".into()],
        )
    };
    let career = if let Some(c) = input.career.as_ref().filter(|c| {
        c.sample_count >= 100 && c.unique_days >= 10 && c.kpm_mad.is_some_and(|m| m > 0.)
    }) {
        let mad = c.kpm_mad.unwrap();
        let z = 0.6745 * (input.current_kpm - c.kpm_median) / mad;
        let sustained = c.recent_kpm.len() >= 3
            && c.recent_kpm[c.recent_kpm.len() - 3..]
                .iter()
                .all(|v| *v > c.kpm_median + 4. * mad);
        verdict(
            input,
            "career_deviation",
            "CAREER",
            if z >= 8. && sustained {
                Decision::CheatLikely
            } else if z >= 4. {
                Decision::Suspicious
            } else {
                Decision::Normal
            },
            vec![
                format!("robust_z:{z:.2}"),
                if sustained {
                    "SUSTAINED"
                } else {
                    "NOT_SUSTAINED"
                }
                .into(),
            ],
        )
    } else {
        verdict(
            input,
            "career_deviation",
            "CAREER",
            Decision::Unknown,
            vec!["INSUFFICIENT_CLEAN_CAREER".into()],
        )
    };
    let change = if let Some(series) = input.change_series.as_ref().filter(|s| s.len() >= 20) {
        let history = &series[..series.len() - 3];
        let recent = &series[series.len() - 3..];
        let mean = history.iter().sum::<f64>() / history.len() as f64;
        let scale =
            (history.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / history.len() as f64).sqrt();
        if scale < 0.1 {
            verdict(
                input,
                "change_point",
                "CHANGE_POINT",
                Decision::Unknown,
                vec!["CAREER_VARIANCE_UNRESOLVED".into()],
            )
        } else {
            let cusum = recent.iter().fold(0f64, |sum, value| {
                (sum + (value - mean) / scale - 0.5).max(0.)
            });
            let persistent = recent.iter().all(|value| *value > mean + 2. * scale);
            verdict(
                input,
                "change_point",
                "CHANGE_POINT",
                if cusum >= 15. && persistent {
                    Decision::CheatLikely
                } else if cusum >= 8. && persistent {
                    Decision::Suspicious
                } else {
                    Decision::Normal
                },
                vec![format!("cusum:{cusum:.2}")],
            )
        }
    } else {
        verdict(
            input,
            "change_point",
            "CHANGE_POINT",
            Decision::Unknown,
            vec!["INSUFFICIENT_ORDERED_HISTORY".into()],
        )
    };
    let persistence = if input.statistical["status"] != "READY" {
        verdict(
            input,
            "persistence",
            "PERSISTENCE",
            Decision::Unknown,
            vec!["NO_CURRENT_ASSESSMENT".into()],
        )
    } else {
        verdict(
            input,
            "persistence",
            "PERSISTENCE",
            if input.independent_episodes >= 3 {
                Decision::CheatLikely
            } else if input.independent_episodes >= 2 {
                Decision::Suspicious
            } else {
                Decision::Normal
            },
            vec![format!(
                "independent_episodes:{}",
                input.independent_episodes
            )],
        )
    };
    vec![tempo, precision, career, change, persistence]
}
pub fn assess(input: &Input, quality: &Quality) -> Committee {
    let mut result = vote(expert_verdicts(input), quality.veto());
    if direct_kick(&result.verdicts, input.current_kpm) {
        result.decision = "KICK_CANDIDATE".into()
    } else if result.decision == "NORMAL" && input.current_kpm > 2. {
        result.decision = "WATCH".into()
    }
    result
}
pub fn statistical_anomaly(assessment: &Value) -> bool {
    assessment["status"] == "READY"
        && assessment["modelVersion"] == MODEL_VERSION
        && assessment["featureVersion"] == FEATURE_VERSION
        && (assessment["metrics"].as_array().is_some_and(|rows| {
            rows.iter().any(|m| {
                m["code"] != "maxKillDistanceWeapon"
                    && m["extremenessPercentile"].as_f64().unwrap_or(0.) >= 0.9
            })
        }) || assessment
            .pointer("/committee/verdicts")
            .and_then(Value::as_array)
            .is_some_and(|votes| {
                votes.iter().any(|v| {
                    v["modelId"] != "persistence"
                        && ["SUSPICIOUS", "CHEAT_LIKELY"]
                            .contains(&v["decision"].as_str().unwrap_or(""))
                })
            }))
}
pub fn should_save(assessment: &Value) -> bool {
    assessment["status"] == "READY"
        && (statistical_anomaly(assessment)
            || assessment
                .get("committee")
                .is_some_and(|c| !c.is_null() && c["decision"] != "NORMAL"))
}
