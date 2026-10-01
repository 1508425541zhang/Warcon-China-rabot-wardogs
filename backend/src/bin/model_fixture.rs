use serde::Deserialize;
use std::{env, fs, path::Path};
use warcon_backend::model::Predictor;
#[derive(Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    label: String,
    rows: Vec<Vec<f32>>,
    masks: Vec<Vec<f32>>,
    score: f64,
}
fn main() -> anyhow::Result<()> {
    let root = env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("Usage: model_fixture ARTIFACTS_DIRECTORY"))?;
    let root = Path::new(&root);
    let predictor = Predictor::load(root)?;
    let fixture: Fixture = serde_json::from_slice(&fs::read(root.join("parity.json"))?)?;
    for case in fixture.cases {
        anyhow::ensure!(
            case.rows.len() == predictor.manifest.window_steps
                && case.masks.len() == case.rows.len(),
            "Invalid fixture rows"
        );
        let mut input = Vec::new();
        for (row, mask) in case.rows.iter().zip(&case.masks) {
            anyhow::ensure!(
                row.len() == predictor.manifest.features && mask.len() == row.len(),
                "Invalid fixture columns"
            );
            input.extend_from_slice(row);
            input.extend_from_slice(mask);
        }
        let start = std::time::Instant::now();
        let result = predictor.score(&input)?;
        let error = (result.score - case.score).abs();
        anyhow::ensure!(error <= 1e-4, "Parity failed: {error}");
        println!(
            "{}",
            serde_json::json!({"case":case.label,"expected":case.score,"actual":result.score,"absoluteError":error,"elapsedMs":start.elapsed().as_millis(),"runtime":"rust"})
        );
    }
    Ok(())
}
