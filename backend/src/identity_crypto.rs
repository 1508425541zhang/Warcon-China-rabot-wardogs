//! Compatibility with existing Better Auth password, factor and recovery records.
use crate::error::{ApiError, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit},
};
use hmac::{Hmac, Mac};
use rand::{Rng, RngCore, rngs::OsRng};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::sync::{Arc, OnceLock};
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;
use unicode_normalization::UnicodeNormalization;

fn failure() -> ApiError {
    ApiError::new(
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "identity_crypto",
        "Identity operation failed.",
    )
}
pub fn random_token(bytes: usize) -> String {
    let mut data = vec![0; bytes];
    OsRng.fill_bytes(&mut data);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}
pub fn random_ascii(length: usize) -> String {
    const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    (0..length)
        .map(|_| ABC[OsRng.gen_range(0..ABC.len())] as char)
        .collect()
}
pub fn password_valid(password: &str) -> Result<()> {
    if !(10..=200).contains(&password.encode_utf16().count()) {
        return Err(ApiError::bad("Password must be 10–200 characters."));
    }
    Ok(())
}
pub fn username_valid(username: &str) -> Result<String> {
    let name = username.trim();
    let b = name.as_bytes();
    if !(2..=32).contains(&b.len())
        || !b[0].is_ascii_alphanumeric()
        || !b
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(c))
    {
        return Err(ApiError::bad(
            "Username must be 2–32 characters: letters, digits, dot, dash, underscore.",
        ));
    }
    Ok(name.to_lowercase())
}
fn derive(password: &str, salt: &str) -> anyhow::Result<[u8; 64]> {
    let mut key = [0; 64];
    let normalized: String = password.nfkc().collect();
    // Better Auth passes the hexadecimal salt as UTF-8 text to scrypt.
    scrypt::scrypt(
        normalized.as_bytes(),
        salt.as_bytes(),
        &scrypt::Params::new(14, 16, 1, 64)?,
        &mut key,
    )?;
    Ok(key)
}
fn budget() -> Arc<Semaphore> {
    static BUDGET: OnceLock<Arc<Semaphore>> = OnceLock::new();
    BUDGET.get_or_init(|| Arc::new(Semaphore::new(4))).clone()
}
pub async fn hash_password(password: String) -> Result<String> {
    password_valid(&password)?;
    let permit = budget().acquire_owned().await.map_err(|_| failure())?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut bytes = [0; 16];
        OsRng.fill_bytes(&mut bytes);
        let salt = hex::encode(bytes);
        derive(&password, &salt)
            .map(|key| format!("{salt}:{}", hex::encode(key)))
            .map_err(|_| failure())
    })
    .await
    .map_err(|_| failure())?
}
pub async fn verify_password(password: String, encoded: Option<String>) -> Result<bool> {
    // Invalid/missing accounts incur the same bounded scrypt work as an incorrect password.
    if password.encode_utf16().count() > 200 {
        return Ok(false);
    }
    let permit = budget().acquire_owned().await.map_err(|_| failure())?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let parts = encoded
            .as_deref()
            .and_then(|s| s.split_once(':'))
            .filter(|(s, k)| {
                s.len() == 32 && k.len() == 128 && s.bytes().all(|b| b.is_ascii_hexdigit())
            });
        let salt = parts
            .map(|p| p.0)
            .unwrap_or("00000000000000000000000000000000");
        let expected = parts
            .and_then(|p| hex::decode(p.1).ok())
            .unwrap_or_else(|| vec![0; 64]);
        let actual = derive(&password, salt).map_err(|_| failure())?;
        Ok(parts.is_some() && bool::from(actual.as_slice().ct_eq(&expected)))
    })
    .await
    .map_err(|_| failure())?
}
pub fn factor_encrypt(secret: &str, plaintext: &str) -> Result<String> {
    let key = Sha256::digest(secret.as_bytes());
    let cipher = XChaCha20Poly1305::new(&key);
    let mut nonce = [0; 24];
    OsRng.fill_bytes(&mut nonce);
    let encrypted = cipher
        .encrypt(XNonce::from_slice(&nonce), plaintext.as_bytes())
        .map_err(|_| failure())?;
    Ok(hex::encode(
        [nonce.as_slice(), encrypted.as_slice()].concat(),
    ))
}
pub fn factor_decrypt(secret: &str, encoded: &str) -> Result<String> {
    // A single BETTER_AUTH_SECRET corresponds to Better Auth secret version zero.
    let encoded = encoded.strip_prefix("$ba$0$").unwrap_or(encoded);
    let bytes = hex::decode(encoded).map_err(|_| failure())?;
    if bytes.len() < 40 {
        return Err(failure());
    }
    let key = Sha256::digest(secret.as_bytes());
    let cipher = XChaCha20Poly1305::new(&key);
    let plaintext = cipher
        .decrypt(XNonce::from_slice(&bytes[..24]), &bytes[24..])
        .map_err(|_| failure())?;
    String::from_utf8(plaintext).map_err(|_| failure())
}
pub fn totp(secret: &str, seconds: i64) -> String {
    let mut mac = <Hmac<Sha1> as Mac>::new_from_slice(secret.as_bytes()).expect("HMAC key");
    mac.update(&((seconds.max(0) / 30) as u64).to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = (hash[19] & 15) as usize;
    let n = u32::from_be_bytes(hash[offset..offset + 4].try_into().unwrap()) & 0x7fff_ffff;
    format!("{:06}", n % 1_000_000)
}
pub fn verify_totp(secret: &str, code: &str, seconds: i64) -> bool {
    let mut valid = false;
    for delta in [-30, 0, 30] {
        valid |= bool::from(
            totp(secret, seconds + delta)
                .as_bytes()
                .ct_eq(code.as_bytes()),
        );
    }
    valid && code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit())
}
pub fn base32(bytes: &[u8]) -> String {
    const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut acc = 0u32;
    let mut bits = 0;
    let mut output = String::new();
    for b in bytes {
        acc = (acc << 8) | *b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            output.push(ABC[((acc >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        output.push(ABC[((acc << (5 - bits)) & 31) as usize] as char);
    }
    output
}
pub fn backup_codes() -> Vec<String> {
    (0..10)
        .map(|_| {
            let s = random_ascii(10);
            format!("{}-{}", &s[..5], &s[5..])
        })
        .collect()
}
pub fn recovery_key() -> String {
    const ABC: &[u8] = b"ABCDEFGHJKMNPQRSTVWXYZ23456789";
    (0..8)
        .map(|_| {
            (0..5)
                .map(|_| ABC[OsRng.gen_range(0..ABC.len())] as char)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("-")
}
pub fn recovery_hash(input: &str) -> String {
    let normalized: String = input
        .to_uppercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| match c {
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        })
        .collect();
    hex::encode(Sha256::digest(normalized.as_bytes()))
}
pub fn signature(secret: &str, value: &str) -> String {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(secret.as_bytes()).expect("HMAC key");
    mac.update(value.as_bytes());
    STANDARD.encode(mac.finalize().into_bytes())
}
pub fn signed(secret: &str, value: &str) -> String {
    format!("{value}.{}", signature(secret, value))
}
pub fn read_signed(headers: &axum::http::HeaderMap, secret: &str, suffix: &str) -> Option<String> {
    let cookie = headers.get("cookie")?.to_str().ok()?;
    for part in cookie.split(';') {
        let (name, value) = part.trim().split_once('=')?;
        if name != format!("warcon.{suffix}") && name != format!("__Secure-warcon.{suffix}") {
            continue;
        }
        let decoded = percent_encoding::percent_decode_str(value)
            .decode_utf8()
            .ok()?;
        let (value, signature) = decoded.rsplit_once('.')?;
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(secret.as_bytes()).ok()?;
        mac.update(value.as_bytes());
        mac.verify_slice(&crate::crypto::decode_base64(signature).ok()?)
            .ok()?;
        return Some(value.into());
    }
    None
}
pub fn safe_path(path: &str, fallback: &str) -> String {
    if path.starts_with('/')
        && !path.starts_with("//")
        && !path
            .chars()
            .any(|c| c.is_whitespace() || c == '\\' || c.is_control())
    {
        path.into()
    } else {
        fallback.into()
    }
}
