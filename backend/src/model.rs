//! Native CPU inference using exactly the existing Float32 artifact format.
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, fs, path::Path};

const WIDTH: usize = 128;
#[derive(Deserialize)]
struct Tensor {
    shape: Vec<usize>,
    offset: usize,
    length: usize,
}
#[derive(Deserialize)]
struct Index {
    tensors: HashMap<String, Tensor>,
}
#[derive(Deserialize, Clone)]
pub struct Manifest {
    pub model_id: String,
    pub checkpoint_sha256: String,
    pub feature_schema: String,
    pub epoch: u32,
    pub channels: usize,
    pub features: usize,
    pub window_steps: usize,
    pub bucket_seconds: usize,
    pub weights_sha256: String,
    pub weights_index_sha256: String,
    pub scaler_sha256: String,
    pub calibration_sha256: String,
}
#[derive(Deserialize)]
pub struct Contract {
    pub features: Vec<String>,
    pub feature_groups: Vec<String>,
    pub window_steps: usize,
    pub center: Vec<f64>,
    pub scale: Vec<f64>,
    pub base: serde_json::Value,
    pub distance_baseline: serde_json::Value,
    pub weapon_class_by_id: serde_json::Value,
}
pub struct Network {
    weights: Vec<f32>,
    index: HashMap<String, Tensor>,
    steps: usize,
    channels: usize,
}
pub struct Predictor {
    pub manifest: Manifest,
    pub manifest_json: serde_json::Value,
    pub calibration: serde_json::Value,
    pub contract: Contract,
    network: Network,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Score {
    pub score: f64,
    pub point_scores: Vec<f64>,
}

pub(crate) fn checked_file(root: &Path, name: &str, hash: &str) -> Result<Vec<u8>> {
    let bytes = fs::read(root.join(name))?;
    ensure!(
        hex::encode(Sha256::digest(&bytes)) == hash,
        "Artifact hash mismatch: {name}"
    );
    Ok(bytes)
}
impl Predictor {
    pub fn load(root: &Path) -> Result<Self> {
        let metadata = fs::read(root.join("manifest.json"))?;
        let manifest: Manifest = serde_json::from_slice(&metadata)?;
        let manifest_json = serde_json::from_slice(&metadata)?;
        let contract: Contract = serde_json::from_slice(&checked_file(
            root,
            "feature_scaler.json",
            &manifest.scaler_sha256,
        )?)?;
        let calibration = serde_json::from_slice(&checked_file(
            root,
            "calibration.json",
            &manifest.calibration_sha256,
        )?)?;
        ensure!(
            manifest.channels == contract.features.len() * 2
                && manifest.features == contract.features.len()
                && contract.feature_groups.len() == manifest.features,
            "Feature dimensions mismatch"
        );
        ensure!(
            contract.center.len() == manifest.features
                && contract.scale.len() == manifest.features
                && contract.center.iter().all(|v| v.is_finite())
                && contract.scale.iter().all(|v| v.is_finite() && *v > 0.),
            "Invalid scaler"
        );
        ensure!(
            manifest.window_steps == 60
                && contract.window_steps == 60
                && manifest.bucket_seconds == 30,
            "Unsupported temporal contract"
        );
        let network = Network::load(root, &manifest)?;
        Ok(Self {
            manifest,
            manifest_json,
            calibration,
            contract,
            network,
        })
    }
    pub fn score(&self, input: &[f32]) -> Result<Score> {
        let pred = self.network.forward(input)?;
        let f = self.manifest.features;
        let c = 2 * f;
        let weights: Vec<f64> = self
            .contract
            .feature_groups
            .iter()
            .map(|g| match g.as_str() {
                "combat" => 1.,
                "weapon" => 0.6,
                "economy" => 0.3,
                _ => 0.,
            })
            .collect();
        let mut totals = vec![0.; f];
        let mut counts = vec![0usize; f];
        let mut total = 0.;
        let mut count = 0usize;
        let mut points = Vec::new();
        for t in 0..self.manifest.window_steps {
            let mut a = 0.;
            let mut n = 0;
            for i in 0..f {
                let mask = input[t * c + f + i];
                ensure!(mask == 0. || mask == 1., "Invalid feature mask");
                if mask == 1. && weights[i] > 0. {
                    let d = (pred[t * c + i] as f64 - input[t * c + i] as f64).abs();
                    let h = if d < 1. { 0.5 * d * d } else { d - 0.5 };
                    let e = h * weights[i];
                    totals[i] += e;
                    counts[i] += 1;
                    a += e;
                    n += 1;
                }
            }
            points.push(a / (n.max(1) as f64));
            total += a;
            count += n;
        }
        let mut tails: Vec<f64> = totals
            .iter()
            .zip(counts)
            .map(|(v, n)| v / n.max(1) as f64)
            .collect();
        tails.sort_by(|a, b| b.total_cmp(a));
        let n = 5.min(f);
        let score =
            0.7 * total / count.max(1) as f64 + 0.3 * tails[..n].iter().sum::<f64>() / n as f64;
        ensure!(score.is_finite(), "Nonfinite score");
        Ok(Score {
            score,
            point_scores: points,
        })
    }
}
fn gelu(x: f64) -> f32 {
    let z = x.abs() / std::f64::consts::SQRT_2;
    let t = 1. / (1. + 0.3275911 * z);
    let erf = 1.
        - (((((1.061405429 * t - 1.453152027) * t + 1.421413741) * t - 0.284496736) * t
            + 0.254829592)
            * t)
            * (-z * z).exp();
    (0.5 * x * (1. + if x < 0. { -erf } else { erf })) as f32
}
impl Network {
    fn load(root: &Path, m: &Manifest) -> Result<Self> {
        Self::load_dimensions(
            root,
            &m.weights_sha256,
            &m.weights_index_sha256,
            m.window_steps,
            m.channels,
        )
    }
    pub(crate) fn load_dimensions(
        root: &Path,
        weights_hash: &str,
        index_hash: &str,
        steps: usize,
        channels: usize,
    ) -> Result<Self> {
        ensure!(
            (1..=200).contains(&steps) && (1..=2048).contains(&channels),
            "Invalid model dimensions"
        );
        let raw = checked_file(root, "weights.f32", weights_hash)?;
        ensure!(raw.len() % 4 == 0, "Invalid Float32 artifact length");
        let weights: Vec<f32> = raw
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        ensure!(
            weights.iter().all(|v| v.is_finite()),
            "Nonfinite model weights"
        );
        let index: Index =
            serde_json::from_slice(&checked_file(root, "weights.json", index_hash)?)?;
        for t in index.tensors.values() {
            ensure!(
                t.shape.iter().try_fold(1usize, |a, b| a.checked_mul(*b)) == Some(t.length)
                    && t.offset
                        .checked_add(t.length)
                        .is_some_and(|end| end <= weights.len()),
                "Invalid tensor bounds"
            );
        }
        Ok(Self {
            weights,
            index: index.tensors,
            steps,
            channels,
        })
    }
    fn tensor(&self, name: &str, shape: &[usize]) -> Result<&[f32]> {
        let t = self
            .index
            .get(name)
            .with_context(|| format!("Missing tensor {name}"))?;
        ensure!(t.shape == shape, "Invalid tensor shape {name}");
        Ok(&self.weights[t.offset..t.offset + t.length])
    }
    fn linear(
        &self,
        x: &[f32],
        input: usize,
        output: usize,
        prefix: &str,
        conv: bool,
    ) -> Result<Vec<f32>> {
        let shape = if conv {
            vec![output, input, 1]
        } else {
            vec![output, input]
        };
        let w = self.tensor(&format!("{prefix}.weight"), &shape)?;
        let b = self.tensor(&format!("{prefix}.bias"), &[output])?;
        let mut y = vec![0.; self.steps * output];
        for t in 0..self.steps {
            for o in 0..output {
                let mut value = b[o] as f64;
                for i in 0..input {
                    value += x[t * input + i] as f64 * w[o * input + i] as f64;
                }
                y[t * output + o] = value as f32;
            }
        }
        Ok(y)
    }
    fn norm(&self, x: &[f32], prefix: &str) -> Result<Vec<f32>> {
        let w = self.tensor(&format!("{prefix}.weight"), &[WIDTH])?;
        let b = self.tensor(&format!("{prefix}.bias"), &[WIDTH])?;
        let mut y = vec![0.; x.len()];
        for t in 0..self.steps {
            let row = &x[t * WIDTH..(t + 1) * WIDTH];
            let mean = row.iter().map(|v| *v as f64).sum::<f64>() / WIDTH as f64;
            let variance = row.iter().map(|v| (*v as f64 - mean).powi(2)).sum::<f64>();
            let scale = 1. / (variance / WIDTH as f64 + 1e-5).sqrt();
            for i in 0..WIDTH {
                y[t * WIDTH + i] =
                    ((row[i] as f64 - mean) * scale * w[i] as f64 + b[i] as f64) as f32;
            }
        }
        Ok(y)
    }
    fn attention(&self, x: &[f32], prefix: &str) -> Result<Vec<f32>> {
        let q = self.linear(
            x,
            WIDTH,
            WIDTH,
            &format!("{prefix}.query_projection"),
            false,
        )?;
        let k = self.linear(x, WIDTH, WIDTH, &format!("{prefix}.key_projection"), false)?;
        let v = self.linear(
            x,
            WIDTH,
            WIDTH,
            &format!("{prefix}.value_projection"),
            false,
        )?;
        let mut y = vec![0.; x.len()];
        let mut logits = vec![0f32; self.steps];
        for h in 0..4 {
            for t in 0..self.steps {
                let mut maximum = f64::NEG_INFINITY;
                for s in 0..self.steps {
                    let mut dot = 0f64;
                    for d in 0..32 {
                        dot += q[t * WIDTH + h * 32 + d] as f64 * k[s * WIDTH + h * 32 + d] as f64;
                    }
                    logits[s] = ((dot as f32) as f64 / 32f64.sqrt()) as f32;
                    maximum = maximum.max(logits[s] as f64);
                }
                let mut denominator = 0f64;
                for logit in &mut logits {
                    *logit = (*logit as f64 - maximum).exp() as f32;
                    denominator += *logit as f64;
                }
                for logit in &mut logits {
                    *logit = (*logit as f64 / denominator) as f32;
                }
                for d in 0..32 {
                    let mut value = 0f64;
                    for s in 0..self.steps {
                        value += logits[s] as f64 * v[s * WIDTH + h * 32 + d] as f64;
                    }
                    y[t * WIDTH + h * 32 + d] = value as f32;
                }
            }
        }
        self.linear(&y, WIDTH, WIDTH, &format!("{prefix}.out_projection"), false)
    }
    pub(crate) fn forward(&self, input: &[f32]) -> Result<Vec<f32>> {
        ensure!(
            input.len() == self.steps * self.channels && input.iter().all(|v| v.is_finite()),
            "Invalid model input"
        );
        let conv = self.tensor(
            "embedding.value_embedding.tokenConv.weight",
            &[WIDTH, self.channels, 3],
        )?;
        let pe = self.tensor("embedding.position_embedding.pe", &[1, self.steps, WIDTH])?;
        let mut x = vec![0.; self.steps * WIDTH];
        for t in 0..self.steps {
            for o in 0..WIDTH {
                let mut value = 0f64;
                for i in 0..self.channels {
                    for k in 0..3 {
                        value += input[((t + k + self.steps - 1) % self.steps) * self.channels + i]
                            as f64
                            * conv[(o * self.channels + i) * 3 + k] as f64;
                    }
                }
                x[t * WIDTH + o] = ((value as f32) as f64 + pe[t * WIDTH + o] as f64) as f32;
            }
        }
        for layer in 0..2 {
            let prefix = format!("encoder.attn_layers.{layer}");
            let a = self.attention(&x, &format!("{prefix}.attention"))?;
            for i in 0..x.len() {
                x[i] = (x[i] as f64 + a[i] as f64) as f32;
            }
            x = self.norm(&x, &format!("{prefix}.norm1"))?;
            let mut ff = self.linear(&x, WIDTH, 256, &format!("{prefix}.conv1"), true)?;
            for v in &mut ff {
                *v = gelu(*v as f64);
            }
            let out = self.linear(&ff, 256, WIDTH, &format!("{prefix}.conv2"), true)?;
            for i in 0..x.len() {
                x[i] = (x[i] as f64 + out[i] as f64) as f32;
            }
            x = self.norm(&x, &format!("{prefix}.norm2"))?;
        }
        self.linear(
            &self.norm(&x, "encoder.norm")?,
            WIDTH,
            self.channels,
            "projection",
            false,
        )
    }
}
