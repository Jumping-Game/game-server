use server::errors::ErrorCode;
use server::proto::{env, ClientJoin, Envelope, PROTOCOL_VERSION};

#[test]
fn envelope_roundtrip() {
    let payload = ClientJoin {
        name: "tester".to_string(),
        client_version: None,
        device: None,
        capabilities: None,
    };
    let frame = Envelope::new("join", 1, payload);
    let json = serde_json::to_string(&frame).unwrap();
    let parsed: Envelope<ClientJoin> = env(&json).expect("parse envelope");
    assert_eq!(parsed.pv, PROTOCOL_VERSION);
    assert_eq!(parsed.payload.name, "tester");
}

#[test]
fn envelope_rejects_unknown_fields() {
    let json = r#"{"type":"join","pv":1,"seq":1,"ts":1,"payload":{"name":"bad","extra":1}}"#;
    let err = env::<ClientJoin>(json).expect_err("should fail");
    assert_eq!(err.code, ErrorCode::InvalidState.as_str());
}
