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
