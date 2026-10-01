//! Native IsolationForest and XGBoost inference. Artifact conversion is an offline operation.
use crate::short_features;
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;
#[derive(Deserialize)]
struct Tree {
    left: Vec<i32>,
    right: Vec<i32>,
    feature: Vec<i32>,
    threshold: Vec<f64>,
    #[serde(default)]
    feature_map: Vec<usize>,
    #[serde(default)]
    leaf_depth: Vec<f64>,
    #[serde(default)]
    missing_left: Vec<bool>,
}
#[derive(Deserialize)]
pub struct Model {
    format: String,
    pub algorithm: String,
    features: Vec<String>,
    pub source_model_sha256: String,
    pub readiness: String,
    pub distance_baseline: Value,
    trees: Vec<Tree>,
    #[serde(default)]
    imputer: Vec<f64>,
    #[serde(default)]
    reference_scores: Vec<f64>,
    #[serde(default)]
    denominator: f64,
    #[serde(default)]
    base_score: f64,
}
impl Model {
    pub fn load(path: &Path) -> Result<Self> {
        let meta: Value =
            serde_json::from_slice(&std::fs::read(path.with_extension("manifest.json"))?)?;
        let bytes = std::fs::read(path)?;
        ensure!(bytes.len() <= 16 * 1024 * 1024, "Short model too large");
        ensure!(
            hex::encode(Sha256::digest(&bytes)) == meta["model_sha256"],
            "Short model checksum mismatch"
        );
        let m: Self = serde_json::from_slice(&bytes)?;
        ensure!(
            m.source_model_sha256 == meta["source_model_sha256"]
                && m.algorithm == meta["algorithm"],
            "Short model identity mismatch"
        );
        m.validate()?;
        Ok(m)
    }
    fn validate(&self) -> Result<()> {
        ensure!(
            self.format == "warcon-short-native-v1"
                && self.features == short_features::FEATURES
                && ["IsolationForest", "XGBoost"].contains(&self.algorithm.as_str()),
            "Unsupported short model"
        );
        ensure!(
            !self.trees.is_empty() && self.trees.len() <= 256,
            "Invalid forest size"
        );
        if self.algorithm == "IsolationForest" {
            ensure!(
                self.imputer.len() == 24
                    && self.imputer.iter().all(|v| v.is_finite())
                    && self.reference_scores.len() >= 1
                    && self.reference_scores.len() <= 1_000_000
                    && self.reference_scores.iter().all(|v| v.is_finite())
                    && self.reference_scores.windows(2).all(|v| v[0] <= v[1])
                    && self.denominator.is_finite()
                    && self.denominator >= 0.,
                "Invalid isolation contract"
            )
        } else {
            ensure!(
                self.base_score > 0. && self.base_score < 1.,
                "Invalid XGBoost base score"
            )
        }
        for tree in &self.trees {
            let n = tree.left.len();
            ensure!(
                (1..=8191).contains(&n)
                    && tree.right.len() == n
                    && tree.feature.len() == n
                    && tree.threshold.len() == n
                    && tree.threshold.iter().all(|v| v.is_finite()),
                "Invalid tree dimensions"
            );
            if self.algorithm == "IsolationForest" {
                ensure!(
                    tree.leaf_depth.len() == n
                        && tree.leaf_depth.iter().all(|v| v.is_finite() && *v >= 0.)
                        && !tree.feature_map.is_empty()
                        && tree.feature_map.iter().all(|i| *i < 24),
                    "Invalid isolation tree"
                )
            } else {
                ensure!(tree.missing_left.len() == n, "Invalid XGBoost tree")
            }
            let mut seen = vec![false; n];
            let mut stack = vec![0usize];
            while let Some(i) = stack.pop() {
                ensure!(!seen[i], "Tree cycle or shared branch");
                seen[i] = true;
                if tree.left[i] == -1 {
                    ensure!(tree.right[i] == -1, "Invalid tree leaf")
                } else {
                    let f = tree.feature[i];
                    let bound = if self.algorithm == "IsolationForest" {
                        tree.feature_map.len()
                    } else {
                        24
                    };
                    ensure!(
                        f >= 0
                            && (f as usize) < bound
                            && tree.left[i] >= 0
                            && tree.right[i] >= 0
                            && (tree.left[i] as usize) < n
                            && (tree.right[i] as usize) < n,
                        "Invalid split"
                    );
                    stack.push(tree.right[i] as usize);
                    stack.push(tree.left[i] as usize);
                }
            }
            ensure!(seen.iter().all(|v| *v), "Unreachable tree nodes");
        }
        Ok(())
    }
    pub fn score(&self, input: &[f32; 24]) -> Result<f64> {
        ensure!(
            input.iter().all(|v| v.is_finite() || v.is_nan()),
            "Invalid short model input"
        );
        let mut x = *input;
        if self.algorithm == "IsolationForest" {
            for (i, v) in x.iter_mut().enumerate() {
                if v.is_nan() {
                    *v = self.imputer[i] as f32
                }
            }
            let mut depth = 0.;
            for tree in &self.trees {
                let mut node = 0;
                while tree.left[node] != -1 {
                    let feature = tree.feature_map[tree.feature[node] as usize];
                    node = if x[feature] as f64 <= tree.threshold[node] {
                        tree.left[node] as usize
                    } else {
                        tree.right[node] as usize
                    };
                }
                depth += tree.leaf_depth[node];
            }
            let score = if self.denominator == 0. {
                0.5
            } else {
                2f64.powf(-depth / self.denominator)
            };
            ensure!(score.is_finite(), "Nonfinite short score");
            Ok(score)
        } else {
            let mut margin = (self.base_score / (1. - self.base_score)).ln() as f32;
            for tree in &self.trees {
                let mut node = 0;
                while tree.left[node] != -1 {
                    let v = x[tree.feature[node] as usize];
                    let left = if v.is_nan() {
                        tree.missing_left[node]
                    } else {
                        v < tree.threshold[node] as f32
                    };
                    node = if left {
                        tree.left[node] as usize
                    } else {
                        tree.right[node] as usize
                    };
                }
                margin += tree.threshold[node] as f32;
            }
            let score = 1f32 / (1f32 + (-margin).exp());
            ensure!(score.is_finite(), "Nonfinite short score");
            Ok(score as f64)
        }
    }
    pub fn reference_samples(&self) -> usize {
        self.reference_scores.len()
    }
    pub fn threshold(&self, q: f64) -> Option<f64> {
        if self.reference_scores.is_empty() {
            None
        } else {
            Some(
                self.reference_scores
                    [((self.reference_scores.len() - 1) as f64 * q).ceil() as usize],
            )
        }
    }
    pub fn predict(&self, request: &Value) -> Result<Value> {
        let start = std::time::Instant::now();
        let events = short_features::events(&request["events"])?;
        let player = request["player_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Player required"))?;
        let clock = request["decision_game_seconds"]
            .as_f64()
            .filter(|v| v.is_finite())
            .ok_or_else(|| anyhow::anyhow!("Finite game clock required"))?;
        let received = short_features::utc(&request["decision_received_utc"])?;
        if !short_features::has_source(&events, player, clock, received) {
            return Ok(
                json!({"ShortRisk":null,"status":"insufficient_data","next_update_seconds":10}),
            );
        }
        let input =
            short_features::vector(&events, player, clock, received, &self.distance_baseline)?;
        let score = self.score(&input)?;
        let mut result = if self.algorithm == "IsolationForest" {
            let rank = self.reference_scores.partition_point(|v| *v < score) as f64
                / self.reference_scores.len() as f64;
            let warning = self.threshold(0.996).unwrap();
            let kick = self.threshold(0.999).unwrap();
            json!({"ShortRisk":rank,"anomaly_score":score,"percentile":rank*100.,"threshold_percentile":99.6,"threshold_score":warning,"tail_candidate":score>warning,"kick_threshold_score":kick,"kick_candidate":score>kick,"status":"unsupervised_shadow","meaning":"reference distribution percentile, not cheating probability"})
        } else {
            json!({"ShortRisk":score,"status":"experimental"})
        };
        result["calibrated"] = json!(false);
        result["next_update_seconds"] = json!(10);
        result["inference_ms"] = json!(start.elapsed().as_secs_f64() * 1000.);
        result["feature_count"] = json!(24);
        Ok(result)
    }
}
