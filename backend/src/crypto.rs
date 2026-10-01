//! Byte-compatible v1 AES-256-GCM envelopes used by the existing Bun backend.
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD},
};
use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};

pub fn decode_base64(value: &str) -> anyhow::Result<Vec<u8>> {
    for engine in [&STANDARD, &STANDARD_NO_PAD, &URL_SAFE, &URL_SAFE_NO_PAD] {
        if let Ok(v) = engine.decode(value) {
            return Ok(v);
        }
    }
    anyhow::bail!("Invalid base64")
}
pub fn decode_key(value: &str) -> anyhow::Result<[u8; 32]> {
    decode_base64(value)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Encryption key must be 32 bytes"))
}
pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}
pub fn encrypt_secret(key: &str, plaintext: &str) -> anyhow::Result<String> {
    let key = decode_key(key)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| anyhow::anyhow!("Invalid key"))?;
    let mut iv = [0u8; 12];
    OsRng.fill_bytes(&mut iv);
    let ct = cipher
        .encrypt(Nonce::from_slice(&iv), plaintext.as_bytes())
        .map_err(|_| anyhow::anyhow!("Encryption failed"))?;
    Ok(format!(
        "v1.{}.{}",
        STANDARD.encode(iv),
        STANDARD.encode(ct)
    ))
}
pub fn decrypt_secret(key: &str, blob: &str) -> anyhow::Result<String> {
    let fields: Vec<_> = blob.split('.').collect();
    anyhow::ensure!(
        fields.len() == 3 && fields[0] == "v1",
        "Unknown secret format"
    );
    let key = decode_key(key)?;
    let iv = decode_base64(fields[1])?;
    let ct = decode_base64(fields[2])?;
    anyhow::ensure!(iv.len() == 12 && ct.len() >= 16, "Invalid secret envelope");
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| anyhow::anyhow!("Invalid key"))?;
    let text = cipher
        .decrypt(Nonce::from_slice(&iv), ct.as_ref())
        .map_err(|_| anyhow::anyhow!("Decryption failed"))?;
    Ok(String::from_utf8(text)?)
}
pub fn mint_token() -> String {
    let mut b = [0u8; 32];
    OsRng.fill_bytes(&mut b);
    format!("wck_{}", URL_SAFE_NO_PAD.encode(b))
}
pub fn parse_bearer(header: &str) -> Option<&str> {
    if header.starts_with(char::is_whitespace) {
        return None;
    }
    let mut parts = header.split_whitespace();
    let scheme = parts.next()?;
    let token = parts.next()?;
    if !scheme.eq_ignore_ascii_case("Bearer")
        || parts.next().is_some()
        || token.len() != 47
        || !token.starts_with("wck_")
    {
        return None;
    }
    token[4..]
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        .then_some(token)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn envelope_rejects_tampering() {
        let key = STANDARD.encode([7u8; 32]);
        let blob = encrypt_secret(&key, "密码-with-✓").unwrap();
        assert_eq!(decrypt_secret(&key, &blob).unwrap(), "密码-with-✓");
        assert!(decrypt_secret(&STANDARD.encode([8u8; 32]), &blob).is_err());
        assert!(decrypt_secret(&key, "v1.YQ==.Yg==").is_err());
    }
    #[test]
    fn key_format_matches_bun() {
        let t = mint_token();
        assert_eq!(t.len(), 47);
        assert_eq!(parse_bearer(&format!("Bearer {t}")), Some(t.as_str()));
        assert!(parse_bearer(&format!("Bearer {t} ignored")).is_none());
    }
}
