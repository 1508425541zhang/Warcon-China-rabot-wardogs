//! HTTP RCON transport: pinned resolution, bounded bodies, no command retries.
use reqwest::{
    Client,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    time::{Duration, Instant},
};
use tokio::net::lookup_host;
use url::Url;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameTarget {
    pub host: String,
    pub port: u16,
    pub scheme: String,
    pub addresses: Option<Vec<IpAddr>>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameRequest {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub insecure_tls: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: HashMap<String, String>,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressKind {
    Public,
    Private,
    LinkLocal,
}
pub fn classify_address(ip: IpAddr) -> AddressKind {
    use AddressKind::*;
    match ip {
        IpAddr::V4(v) => {
            let [a, b, c, _] = v.octets();
            if a == 169 && b == 254 {
                return LinkLocal;
            }
            if [0, 10, 127].contains(&a)
                || (a == 100 && (64..=127).contains(&b))
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && b == 168)
                || (a == 192 && b == 0 && [0, 2].contains(&c))
                || (a == 198 && [18, 19].contains(&b))
                || (a == 198 && b == 51 && c == 100)
                || (a == 203 && b == 0 && c == 113)
                || a >= 224
            {
                Private
            } else {
                Public
            }
        }
        IpAddr::V6(v) => {
            let g = v.segments();
            let embedded = (g[..5].iter().all(|x| *x == 0) && [0, 0xffff].contains(&g[5]))
                || (g[0] == 0x64 && g[1] == 0xff9b && g[2..6].iter().all(|x| *x == 0));
            if embedded {
                if g[..6].iter().all(|x| *x == 0) && g[6] == 0 && g[7] <= 1 {
                    return Private;
                }
                return classify_address(IpAddr::V4(std::net::Ipv4Addr::new(
                    (g[6] >> 8) as u8,
                    g[6] as u8,
                    (g[7] >> 8) as u8,
                    g[7] as u8,
                )));
            }
            if g[0] & 0xffc0 == 0xfe80 {
                LinkLocal
            } else if g[0] & 0xfe00 == 0xfc00
                || g[0] & 0xff00 == 0xff00
                || (g[0] == 0x2001 && g[1] == 0xdb8)
            {
                Private
            } else {
                Public
            }
        }
    }
}
pub fn is_private(ip: IpAddr) -> bool {
    classify_address(ip) != AddressKind::Public
}
pub fn normalise_host(raw: &str) -> anyhow::Result<String> {
    let host = raw.trim().to_ascii_lowercase();
    let bare = host
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(&host);
    if bare.parse::<std::net::Ipv6Addr>().is_ok() {
        return Ok(bare.to_owned());
    }
    anyhow::ensure!(
        !host.is_empty()
            && host.len() <= 253
            && host.split('.').all(|label| {
                let b = label.as_bytes();
                !b.is_empty()
                    && b[0].is_ascii_alphanumeric()
                    && b[b.len() - 1].is_ascii_alphanumeric()
                    && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'-')
            }),
        "Invalid host"
    );
    Ok(host)
}
pub fn game_path(raw: &str) -> anyhow::Result<String> {
    anyhow::ensure!(
        raw.starts_with('/') && !raw.contains(['\r', '\n', '\\']),
        "Invalid game path"
    );
    let url = Url::parse("http://game.invalid")?.join(raw)?;
    let path = url.path();
    anyhow::ensure!(
        url.host_str() == Some("game.invalid")
            && url.fragment().is_none()
            && path.starts_with("/v1/")
            && !path.contains("//")
            && !path.contains("..")
            && !path.to_ascii_lowercase().contains("%2e"),
        "Invalid game path"
    );
    Ok(format!(
        "{}{}",
        path,
        url.query().map(|q| format!("?{q}")).unwrap_or_default()
    ))
}
pub async fn resolve_target(
    host: &str,
    port: u16,
    scheme: &str,
    allow_private: bool,
) -> anyhow::Result<GameTarget> {
    anyhow::ensure!(
        port > 0 && ["http", "https"].contains(&scheme),
        "Invalid game target"
    );
    let bare = normalise_host(host)?;
    let mut addresses: Vec<_> = if let Ok(ip) = bare.parse::<IpAddr>() {
        vec![ip]
    } else {
        lookup_host((bare.as_str(), port))
            .await?
            .map(|a| a.ip())
            .collect()
    };
    anyhow::ensure!(
        !addresses.is_empty()
            && addresses.iter().all(|ip| match classify_address(*ip) {
                AddressKind::Public => true,
                AddressKind::Private => allow_private,
                AddressKind::LinkLocal => false,
            }),
        "Host policy denied target"
    );
    addresses.sort_by_key(|ip| ip.is_ipv6());
    let mut seen = std::collections::HashSet::new();
    addresses.retain(|ip| seen.insert(*ip));
    Ok(GameTarget {
        host: bare,
        port,
        scheme: scheme.into(),
        addresses: Some(addresses),
    })
}
pub async fn game_request(target: &GameTarget, init: &GameRequest) -> anyhow::Result<GameResponse> {
    let path = game_path(&init.path)?;
    anyhow::ensure!(
        init.path.starts_with('/')
            && !init.path.starts_with("//")
            && !init.path.contains(['\r', '\n']),
        "Invalid game path"
    );
    let authority = if target.host.parse::<std::net::Ipv6Addr>().is_ok() {
        format!("[{}]", target.host)
    } else {
        target.host.clone()
    };
    let url = Url::parse(&format!(
        "{}://{}:{}{}",
        target.scheme, authority, target.port, path
    ))?;
    anyhow::ensure!(
        url.host_str().map(|host| host.trim_matches(['[', ']'])) == Some(target.host.as_str())
            && url.port_or_known_default() == Some(target.port),
        "Unexpected game authority"
    );
    let budget = Duration::from_millis(init.timeout_ms.unwrap_or(10000).clamp(1, 60000));
    let start = Instant::now();
    let addresses = target
        .addresses
        .as_ref()
        .filter(|a| !a.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Pinned addresses required"))?;
    anyhow::ensure!(
        addresses
            .iter()
            .all(|ip| classify_address(*ip) != AddressKind::LinkLocal),
        "Link-local target denied"
    );
    for (i, ip) in addresses.iter().enumerate() {
        let remaining = budget
            .checked_sub(start.elapsed())
            .ok_or_else(|| anyhow::anyhow!("Game request timed out"))?;
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .timeout(remaining)
            .danger_accept_invalid_certs(init.insecure_tls)
            .resolve(&target.host, SocketAddr::new(*ip, target.port))
            .build()?;
        let mut headers = HeaderMap::new();
        headers.insert("user-agent", HeaderValue::from_static("warcon/0.3"));
        headers.insert(
            "accept",
            HeaderValue::from_static("application/json, text/plain, */*"),
        );
        for (k, v) in &init.headers {
            anyhow::ensure!(!k.eq_ignore_ascii_case("host"), "Host override denied");
            headers.insert(
                HeaderName::from_bytes(k.as_bytes())?,
                HeaderValue::from_str(v)?,
            );
        }
        let mut request = client
            .request(init.method.parse()?, url.clone())
            .headers(headers);
        if let Some(body) = &init.body {
            request = request.body(body.clone())
        }
        match request.send().await {
            Ok(response) => {
                let status = response.status();
                let headers = response
                    .headers()
                    .iter()
                    .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.to_string(), v.to_owned())))
                    .collect();
                let mut response = response;
                let mut bytes = Vec::new();
                while let Some(chunk) = response.chunk().await? {
                    anyhow::ensure!(
                        bytes.len() + chunk.len() <= 8 * 1024 * 1024,
                        "Game response exceeds limit"
                    );
                    bytes.extend_from_slice(&chunk);
                }
                return Ok(GameResponse {
                    status: status.as_u16(),
                    status_text: status.canonical_reason().unwrap_or("").into(),
                    headers,
                    text: String::from_utf8(bytes)?,
                });
            }
            Err(error) => {
                // Only a definite refusal before connect can try another validated address.
                // TLS, timeouts, response errors and reset sockets are never resent.
                let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(&error);
                let mut refused = false;
                while let Some(e) = cause {
                    if let Some(e) = e.downcast_ref::<std::io::Error>() {
                        refused = e.kind() == std::io::ErrorKind::ConnectionRefused;
                    }
                    cause = e.source();
                }
                if !error.is_connect() || !refused || i + 1 == addresses.len() {
                    anyhow::bail!("Could not reach game server")
                }
            }
        }
    }
    anyhow::bail!("Could not reach game server")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blocked_address_ranges() {
        for ip in [
            "127.0.0.1",
            "10.0.0.2",
            "169.254.169.254",
            "100.64.0.1",
            "::1",
            "::ffff:127.0.0.1",
            "fd00::1",
            "::127.0.0.1",
            "64:ff9b::7f00:1",
            "2001:db8::1",
            "192.0.2.1",
            "198.18.1.1",
            "198.51.100.1",
            "203.0.113.1",
        ] {
            assert!(is_private(ip.parse().unwrap()));
        }
        assert!(!is_private("8.8.8.8".parse().unwrap()));
    }
    #[tokio::test]
    async fn rejects_private_and_bad_paths() {
        for host in [
            "169.254.169.254",
            "fe80::1",
            "::ffff:169.254.169.254",
            "64:ff9b::a9fe:a9fe",
        ] {
            assert!(
                resolve_target(host, 80, "http", true).await.is_err(),
                "Link-local must never be permitted: {host}"
            );
        }
        for path in [
            "//evil.test/v1/a",
            "/v1/../config",
            "/v1/%2e%2e/config",
            "/v1/a#token",
            "/v1//a",
            "/v1/a\\b",
        ] {
            assert!(game_path(path).is_err());
        }
        assert_eq!(
            game_path("/v1/status?name=a%20b").unwrap(),
            "/v1/status?name=a%20b"
        );
        assert_eq!(normalise_host(" EXAMPLE.org ").unwrap(), "example.org");
        assert!(
            resolve_target("127.0.0.1", 80, "http", false)
                .await
                .is_err()
        );
        assert!(
            resolve_target("example.com/path", 80, "http", false)
                .await
                .is_err()
        );
    }
}
