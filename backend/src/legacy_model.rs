//! 27-channel model with the original mask map, Float32 loss and checkpoint identity.
use crate::{
    legacy_features as features,
    model::{Network, Score, checked_file},
};
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::HashMap, path::Path};
#[derive(Deserialize)]
struct Scale {
    mean: f64,
    std: f64,
}
#[derive(Deserialize)]
struct Scaler {
    channel_order: Vec<String>,
    continuous_channels: HashMap<String, Scale>,
}
pub struct Predictor {
    pub manifest: Value,
    pub calibration: Value,
    scaler: Scaler,
    network: Network,
    pub steps: usize,
}
impl Predictor {
    pub fn load(root: &Path) -> Result<Self> {
        let manifest: Value = serde_json::from_slice(&std::fs::read(root.join("manifest.json"))?)?;
        let hash = |k: &str| {
            manifest[k]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing artifact hash"))
        };
        let scaler: Scaler = serde_json::from_slice(&checked_file(
            root,
            "feature_scaler.json",
            hash("scaler_sha256")?,
        )?)?;
        let calibration = serde_json::from_slice(&checked_file(
            root,
            "calibration.json",
            hash("calibration_sha256")?,
        )?)?;
        let steps = manifest["window_steps"].as_u64().unwrap_or(0) as usize;
        ensure!(
            manifest["epoch"] == 58
                && [60, 200].contains(&steps)
                && scaler.channel_order.len() == 27
                && manifest["feature_schema"] == "warcon-raw-30s-v1",
            "Unsupported model"
        );
        let network = Network::load_dimensions(
            root,
            hash("weights_sha256")?,
            hash("weights_index_sha256")?,
            steps,
            27,
        )?;
        Ok(Self {
            manifest,
            calibration,
            scaler,
            network,
            steps,
        })
    }
    pub fn normalize(&self, rows: &[Value]) -> Result<Vec<f32>> {
        ensure!(rows.len() == self.steps, "Invalid bucket count");
        let mut x = vec![0f32; self.steps * 27];
        for (t, r) in rows.iter().enumerate() {
            for i in 0..14 {
                if features::numeric(&r[features::MASKS[features::MASK_FOR[i]]]) == Some(1.) {
                    let name = &self.scaler.channel_order[i];
                    let scale = self
                        .scaler
                        .continuous_channels
                        .get(name)
                        .ok_or_else(|| anyhow::anyhow!("Missing feature scaler"))?;
                    let v = features::numeric(&r[name])
                        .ok_or_else(|| anyhow::anyhow!("Invalid feature value"))?;
                    ensure!(
                        scale.std > 0. && scale.mean.is_finite() && scale.std.is_finite(),
                        "Invalid feature scaler"
                    );
                    x[t * 27 + i] = ((v - scale.mean) / scale.std) as f32;
                }
            }
            for i in 0..13 {
                let mask = features::numeric(&r[features::MASKS[i]])
                    .ok_or_else(|| anyhow::anyhow!("Invalid feature mask"))?;
                ensure!(mask == 0. || mask == 1., "Invalid feature mask");
                x[t * 27 + 14 + i] = mask as f32;
            }
        }
        ensure!(x.iter().all(|v| v.is_finite()), "Nonfinite model input");
        Ok(x)
    }
    pub fn score(&self, rows: &[Value]) -> Result<Score> {
        let input = self.normalize(rows)?;
        let pred = self.network.forward(&input)?;
        let mut numeric = vec![0f32; self.steps];
        let mut masks = vec![0f32; self.steps];
        let mut counts = vec![0f32; self.steps];
        let mut points = Vec::with_capacity(self.steps);
        for t in 0..self.steps {
            for i in 0..14 {
                let observed = input[t * 27 + 14 + features::MASK_FOR[i]];
                let d = (pred[t * 27 + i] as f64 - input[t * 27 + i] as f64) as f32;
                let square = (d as f64 * d as f64) as f32;
                numeric[t] = (numeric[t] as f64 + square as f64 * observed as f64) as f32;
                counts[t] = (counts[t] as f64 + observed as f64) as f32;
            }
            for i in 14..27 {
                let d = (pred[t * 27 + i] as f64 - input[t * 27 + i] as f64) as f32;
                masks[t] = (masks[t] as f64 + (d as f64 * d as f64) as f32 as f64) as f32;
            }
            points.push(
                ((0.8 * numeric[t] as f64) / f64::from(counts[t]).max(1.)
                    + 0.2 * masks[t] as f64 / 13.) as f32 as f64,
            );
        }
        let sum = |a: &[f32]| a.iter().map(|v| *v as f64).sum::<f64>();
        let score = (0.8 * sum(&numeric) / sum(&counts).max(1.)
            + 0.2 * sum(&masks) / (self.steps * 13) as f64) as f32 as f64;
        ensure!(
            score.is_finite() && points.iter().all(|v| v.is_finite()),
            "Nonfinite model output"
        );
        Ok(Score {
            score,
            point_scores: points,
        })
    }
    pub fn assess(&self, request: &Value) -> Result<Value> {
        let mut rows = features::build(request)?;
        if rows.len() > self.steps {
            rows = rows.split_off(rows.len() - self.steps)
        }
        let age = features::timestamp(&request["sources"]["matches"][0]["ended_at"])?
            .zip(
                rows.last()
                    .map(|r| features::timestamp(&r["bucket_start_utc"]))
                    .transpose()?
                    .flatten(),
            )
            .map(|(a, b)| a - b - 30000);
        let observed = rows.iter().filter(|r| r["bucket_observed"] == 1).count();
        let cash = rows.iter().filter(|r| r["cash_observed"] == 1).count();
        let mut result = json!({"modelId":self.manifest["model_id"],"checkpointSha256":self.manifest["checkpoint_sha256"],"schema":self.manifest["feature_schema"],"requestId":request["requestId"],"stage":"A测","calibrationSha256":self.manifest["calibration_sha256"],"windowSeconds":self.steps*30,"bucketSeconds":30});
        let valid = age.is_some_and(|v| (0..=45000).contains(&v))
            && rows.len() == self.steps
            && rows.iter().all(|r| r["is_active"] == 1)
            && observed as f64 >= self.steps as f64 * 0.8
            && cash as f64 >= self.steps as f64 * 0.75;
        if !valid {
            result["status"] = json!("INSUFFICIENT_DATA");
            result["reason"] = json!(format!(
                "需要同局连续 {} 分钟有效序列、80% 观测及75%现金覆盖。",
                self.steps / 2
            ));
            result["observedBuckets"] = json!(observed);
            result["requiredBuckets"] = json!(self.steps);
            return Ok(result);
        }
        let score = self.score(&rows)?;
        result["status"] = json!("READY");
        result["windowEnd"] = rows.last().unwrap()["bucket_start_utc"].clone();
        result["score"] = json!(score.score);
        result["pointScores"] = json!(score.point_scores);
        Ok(result)
    }
}
