use crate::error::ApiError;
use axum::{
    body::to_bytes,
    extract::{FromRequest, FromRequestParts, Query, Request},
    http::{StatusCode, request::Parts},
};
use serde::de::DeserializeOwned;
use serde_json::Value;
pub struct ApiJson<T>(pub T);
pub fn js_number(value: &Value) -> Option<f64> {
    let n = match value {
        Value::Null => Some(0.),
        Value::Bool(b) => Some(if *b { 1. } else { 0. }),
        Value::Number(n) => n.as_f64(),
        Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                Some(0.)
            } else if ["0x", "0X", "0b", "0B", "0o", "0O"]
                .iter()
                .any(|p| s.starts_with(p))
            {
                let radix = match s.as_bytes()[1].to_ascii_lowercase() {
                    b'x' => 16,
                    b'b' => 2,
                    _ => 8,
                };
                u64::from_str_radix(&s[2..], radix).ok().map(|n| n as f64)
            } else {
                s.parse().ok()
            }
        }
        Value::Array(a) if a.is_empty() => Some(0.),
        Value::Array(a) if a.len() == 1 => js_number(&Value::String(string(&a[0], usize::MAX))),
        _ => None,
    };
    n.filter(|n| n.is_finite())
}
pub fn integer(value: &Value, fallback: i64, min: i64, max: i64) -> i64 {
    let n = match value {
        Value::Null => return fallback,
        Value::String(s) if s.is_empty() => return fallback,
        Value::String(s) => s.trim().parse::<f64>().ok(),
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1. } else { 0. }),
        Value::Array(a) if a.is_empty() => Some(0.),
        Value::Array(a) if a.len() == 1 => string(&a[0], usize::MAX).parse::<f64>().ok(),
        _ => None,
    };
    n.filter(|n| n.is_finite())
        .map(|n| (n.trunc().clamp(min as f64, max as f64)) as i64)
        .unwrap_or(fallback)
}
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}
pub fn string(value: &Value, max: usize) -> String {
    let text = match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Array(v) => v
            .iter()
            .map(|v| string(v, usize::MAX))
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
    };
    crate::feed::truncate(text.trim(), max)
}
impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;
    async fn from_request(request: Request, _state: &S) -> Result<Self, Self::Rejection> {
        if !request
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("application/json"))
        {
            return Err(ApiError::new(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "content_type",
                "Expected application/json body.",
            ));
        }
        let bytes = to_bytes(request.into_body(), 2 * 1024 * 1024)
            .await
            .map_err(|_| {
                ApiError::new(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "body_size",
                    "Request body too large.",
                )
            })?;
        let value: Value = if bytes.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_slice(&bytes).map_err(|_| ApiError::bad("Malformed JSON body."))?
        };
        Ok(Self(
            serde_json::from_value(value).map_err(|_| ApiError::bad("Invalid JSON fields."))?,
        ))
    }
}
pub struct ApiQuery<T>(pub T);
#[derive(Clone, Copy)]
pub struct Peer(pub Option<std::net::SocketAddr>);
impl<S: Send + Sync> FromRequestParts<S> for Peer {
    type Rejection = std::convert::Infallible;
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(
            parts
                .extensions
                .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                .map(|v| v.0),
        ))
    }
}
/// A frontend may forward its resolved client address only with a short-lived signed proof.
pub fn client_address(
    state: &crate::config::AppState,
    peer: Peer,
    headers: &axum::http::HeaderMap,
) -> Option<std::net::IpAddr> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    if let Some(secret) = &state.config.identity.frontend_token {
        let ip = headers
            .get("x-warcon-client-ip")
            .and_then(|h| h.to_str().ok());
        let stamp = headers
            .get("x-warcon-client-time")
            .and_then(|h| h.to_str().ok());
        let proof = headers
            .get("x-warcon-client-proof")
            .and_then(|h| h.to_str().ok());
        if let (Some(ip), Some(stamp), Some(proof)) = (ip, stamp, proof) {
            if let (Ok(address), Ok(seconds), Ok(proof)) = (
                ip.parse::<std::net::IpAddr>(),
                stamp.parse::<i64>(),
                hex::decode(proof),
            ) {
                if seconds.abs_diff(chrono::Utc::now().timestamp()) <= 30 {
                    if let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) {
                        mac.update(format!("{stamp}\n{ip}").as_bytes());
                        if mac.verify_slice(&proof).is_ok() {
                            return Some(address);
                        }
                    }
                }
            }
        }
    }
    peer.0.map(|p| p.ip())
}
impl<S, T> FromRequestParts<S> for ApiQuery<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Query(value) = Query::<T>::from_request_parts(parts, state)
            .await
            .map_err(|_| ApiError::bad("Invalid query parameters."))?;
        Ok(Self(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    #[tokio::test]
    async fn client_address_requires_signed_fresh_proof() {
        let config = crate::config::Config::for_test();
        let mut state = crate::config::AppState {
            runtime: Default::default(),
            db: sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgresql://postgres@127.0.0.1/test")
                .unwrap(),
            config,
        };
        let peer = Peer(Some("127.0.0.1:1234".parse().unwrap()));
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-forwarded-for", "203.0.113.7".parse().unwrap());
        assert_eq!(
            client_address(&state, peer, &headers).unwrap().to_string(),
            "127.0.0.1"
        );
        state.config.identity.frontend_token =
            Some("frontend-test-secret-at-least-32-bytes".into());
        let stamp = chrono::Utc::now().timestamp().to_string();
        headers.insert("x-warcon-client-ip", "203.0.113.7".parse().unwrap());
        headers.insert("x-warcon-client-time", stamp.parse().unwrap());
        let mut mac = Hmac::<Sha256>::new_from_slice(
            state
                .config
                .identity
                .frontend_token
                .as_ref()
                .unwrap()
                .as_bytes(),
        )
        .unwrap();
        mac.update(format!("{stamp}\n203.0.113.7").as_bytes());
        headers.insert(
            "x-warcon-client-proof",
            hex::encode(mac.finalize().into_bytes()).parse().unwrap(),
        );
        assert_eq!(
            client_address(&state, peer, &headers).unwrap().to_string(),
            "203.0.113.7"
        );
        headers.insert("x-warcon-client-ip", "203.0.113.8".parse().unwrap());
        assert_eq!(
            client_address(&state, peer, &headers).unwrap().to_string(),
            "127.0.0.1"
        );
        headers.insert("x-warcon-client-ip", "203.0.113.7".parse().unwrap());
        headers.insert(
            "x-warcon-client-time",
            (chrono::Utc::now().timestamp() - 60)
                .to_string()
                .parse()
                .unwrap(),
        );
        assert_eq!(
            client_address(&state, peer, &headers).unwrap().to_string(),
            "127.0.0.1"
        );
    }
}
