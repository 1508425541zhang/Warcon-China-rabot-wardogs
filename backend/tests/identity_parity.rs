use serde_json::Value;
use warcon_backend::{auth, identity, identity_crypto as crypto, oauth};
#[tokio::test]
async fn existing_better_auth_records_and_enrolment_are_compatible() {
    let f: Value = serde_json::from_str(include_str!("../fixtures/identity.json")).unwrap();
    let secret = f["authSecret"].as_str().unwrap();
    for p in f["passwords"].as_array().unwrap() {
        assert!(
            crypto::verify_password(
                p["password"].as_str().unwrap().into(),
                Some(p["hash"].as_str().unwrap().into())
            )
            .await
            .unwrap()
        );
    }
    assert!(
        !crypto::verify_password(
            "IncorrectPassword".into(),
            Some(f["passwords"][0]["hash"].as_str().unwrap().into())
        )
        .await
        .unwrap()
    );
    assert_eq!(
        crypto::factor_decrypt(secret, f["factor"].as_str().unwrap()).unwrap(),
        f["plaintext"].as_str().unwrap()
    );
    assert!(crypto::factor_decrypt("another-secret", f["factor"].as_str().unwrap()).is_err());
    for row in f["otp"].as_array().unwrap() {
        let seconds = row["seconds"].as_i64().unwrap();
        let code = row["code"].as_str().unwrap();
        assert_eq!(
            crypto::totp(f["plaintext"].as_str().unwrap(), seconds),
            code
        );
        assert!(crypto::verify_totp(
            f["plaintext"].as_str().unwrap(),
            code,
            seconds + 30
        ));
        assert!(!crypto::verify_totp(
            f["plaintext"].as_str().unwrap(),
            "not-a-code",
            seconds
        ));
    }
    for row in f["enrolments"].as_array().unwrap() {
        assert_eq!(
            identity::enrolment(&row["methods"], row["owner"].as_bool().unwrap()),
            row["result"]
        );
    }
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("cookie", f["cookie"].as_str().unwrap().parse().unwrap());
    assert_eq!(
        auth::session_cookie(&headers, secret).unwrap(),
        f["cookieToken"].as_str().unwrap()
    );
    assert_eq!(
        crypto::recovery_hash("abcde-oi123"),
        crypto::recovery_hash("ABCDE-01123")
    );
    for path in [
        "//evil.test",
        "/\\evil",
        "https://evil.test",
        "/ bad",
        "/\n",
    ] {
        assert_eq!(crypto::safe_path(path, "/"), "/");
    }
}
#[test]
fn steam_duplicate_identity_and_unsigned_fields_cannot_change_the_account() {
    let callback = "https://localhost/auth/steam/callback?state=fixture";
    let mut fields = url::form_urlencoded::Serializer::new(String::new());
    fields.extend_pairs([
        ("openid.ns", "http://specs.openid.net/auth/2.0"),
        ("openid.mode", "id_res"),
        (
            "openid.claimed_id",
            "https://steamcommunity.com/openid/id/76561198388453389",
        ),
        (
            "openid.identity",
            "https://steamcommunity.com/openid/id/76561198388453389",
        ),
        ("openid.return_to", callback),
        ("openid.response_nonce", "fixture-nonce"),
        ("openid.assoc_handle", "fixture-assoc"),
        (
            "openid.signed",
            "claimed_id,identity,return_to,response_nonce,assoc_handle",
        ),
    ]);
    let query = fields.finish();
    assert_eq!(
        oauth::steam_fields(&query, callback).unwrap().0,
        "76561198388453389"
    );
    assert!(oauth::steam_fields(&format!("{query}&openid.identity=another"), callback).is_err());
    assert!(oauth::steam_fields(&query, "https://other.test/").is_err());
    assert!(
        oauth::steam_fields(
            &query.replace("claimed_id%2Cidentity", "identity"),
            callback
        )
        .is_err()
    );
}
