use serde_json::{Value, json};
use warcon_backend::{qq_community as c, qq_gateway, qq_protocol as p};
#[test]
fn signed_protocol_and_battle_match_original() {
    let f: Value = serde_json::from_str(include_str!("../fixtures/qq.json")).unwrap();
    let now = f["now"].as_i64().unwrap();
    let self_id = f["self"].as_str().unwrap();
    for case in f["onebot"].as_array().unwrap() {
        assert_eq!(
            serde_json::to_value(p::onebot(&case["input"], self_id, now)).unwrap(),
            case["expected"],
            "{}",
            case["input"]
        )
    }
    for case in f["official"].as_array().unwrap() {
        assert_eq!(
            serde_json::to_value(p::official(&case["input"], self_id, now)).unwrap(),
            case["expected"]
        )
    }
    for case in f["commands"].as_array().unwrap() {
        let (name, arg) = p::command(case["content"].as_str().unwrap());
        assert_eq!(json!({"name":name,"arg":arg}), case["expected"])
    }
    for case in f["battles"].as_array().unwrap() {
        assert_eq!(
            c::battle(
                &case["status"],
                case["roster"].as_array().unwrap(),
                case["page"].as_u64().unwrap() as usize
            )
            .unwrap(),
            case["expected"]
        )
    }
    for case in f["blocks"].as_array().unwrap() {
        assert_eq!(
            c::block(
                case["name"].as_str().unwrap(),
                case["hex"].as_str().unwrap()
            ),
            case["expected"]
        )
    }
    let secret = f["secret"].as_str().unwrap();
    let mut h = axum::http::HeaderMap::new();
    h.insert(
        "x-signature",
        f["hmac"]["signature"].as_str().unwrap().parse().unwrap(),
    );
    assert!(p::verify_onebot(
        secret,
        &h,
        f["hmac"]["body"].as_str().unwrap().as_bytes()
    ));
    assert!(!p::verify_onebot(secret, &h, b"tampered"));
    h.insert(
        "x-signature-timestamp",
        f["time"].as_str().unwrap().parse().unwrap(),
    );
    h.insert(
        "x-signature-ed25519",
        f["signed"]["signature"].as_str().unwrap().parse().unwrap(),
    );
    assert!(p::verify_official(
        secret,
        &h,
        f["signed"]["body"].as_str().unwrap().as_bytes(),
        now
    ));
    assert!(!p::verify_official(secret, &h, b"tampered", now));
    assert!(!p::verify_official(
        secret,
        &h,
        f["signed"]["body"].as_str().unwrap().as_bytes(),
        now + 301000
    ));
    assert_eq!(
        p::validation(secret, "challenge", f["time"].as_str().unwrap()).unwrap(),
        f["validation"]
    );
    assert!(qq_gateway::valid_url("wss://api.bot.qq.com/websocket"));
    assert!(!qq_gateway::valid_url("ws://api.bot.qq.com/websocket"));
    assert!(!qq_gateway::valid_url(
        "wss://api.bot.qq.com.evil.invalid/websocket"
    ));
    assert!(!qq_gateway::valid_url(
        "wss://user@api.bot.qq.com/websocket"
    ));
}
