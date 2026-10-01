#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let target = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("Missing health URL"))?;
    let url = url::Url::parse(&target)?;
    anyhow::ensure!(
        url.host_str()
            .is_some_and(|h| h == "127.0.0.1" || h == "localhost" || h == "[::1]"),
        "Health target must be local"
    );
    let request = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(4))
        .build()?
        .get(url);
    let request = if std::env::args().nth(2).as_deref() == Some("--model") {
        request.bearer_auth(std::env::var("MODEL_API_TOKEN")?)
    } else {
        request
    };
    let response = request.send().await?;
    anyhow::ensure!(response.status().is_success(), "Service is not healthy");
    Ok(())
}
