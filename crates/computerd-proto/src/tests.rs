//! The wire, held to what Magnus sent before the requests were typed: each
//! typed request is the JSON the hand-built `json!` was, and a computerd
//! reads that JSON back as the same request.

use super::guest::GuestRequest;
use super::*;
use serde_json::{json, Value};

/// Each request beside the `json!` Magnus built for it until 2026-11-14.
fn computerd_requests() -> Vec<(Request, Value)> {
    let id = || "c1".to_string();
    vec![
        (Request::Create { id: id() }, json!({ "op": "create", "id": "c1" })),
        (Request::Start { id: id() }, json!({ "op": "start", "id": "c1" })),
        (Request::Hibernate { id: id() }, json!({ "op": "hibernate", "id": "c1" })),
        (Request::Wake { id: id() }, json!({ "op": "wake", "id": "c1" })),
        (Request::DiscardSnapshot { id: id() }, json!({ "op": "discard_snapshot", "id": "c1" })),
        (Request::Backup { id: id() }, json!({ "op": "backup", "id": "c1" })),
        (Request::EgressSocket, json!({ "op": "egress_socket" })),
        (Request::Capacity, json!({ "op": "capacity" })),
        (Request::Whose { uid: 70001 }, json!({ "op": "whose", "uid": 70001 })),
        (Request::RestoreBackup { id: id() }, json!({ "op": "restore_backup", "id": "c1" })),
        (Request::StartBrowser { id: id() }, json!({ "op": "start_browser", "id": "c1" })),
        (Request::ZeroRoom { id: id() }, json!({ "op": "zero_room", "id": "c1" })),
        (Request::DigHoles { id: id() }, json!({ "op": "dig_holes", "id": "c1" })),
        (Request::SealVolume { id: id(), key_b64: "a2V5".into() }, json!({ "op": "seal_volume", "id": "c1", "key_b64": "a2V5" })),
        (Request::UnsealVolume { id: id(), key_b64: "a2V5".into() }, json!({ "op": "unseal_volume", "id": "c1", "key_b64": "a2V5" })),
        (Request::StartHelper { id: id(), read_only: true }, json!({ "op": "start_helper", "id": "c1", "read_only": true })),
        (Request::StartHelper { id: id(), read_only: false }, json!({ "op": "start_helper", "id": "c1", "read_only": false })),
        (Request::Pause { id: id() }, json!({ "op": "pause", "id": "c1" })),
        (Request::Resume { id: id() }, json!({ "op": "resume", "id": "c1" })),
        (Request::Stop { id: id() }, json!({ "op": "stop", "id": "c1" })),
        (Request::Destroy { id: id(), delete_volume: true }, json!({ "op": "destroy", "id": "c1", "delete_volume": true })),
        (Request::Destroy { id: id(), delete_volume: false }, json!({ "op": "destroy", "id": "c1", "delete_volume": false })),
        (Request::EraseVolume { id: id() }, json!({ "op": "erase_volume", "id": "c1" })),
        (Request::Status { id: id() }, json!({ "op": "status", "id": "c1" })),
        (Request::List, json!({ "op": "list" })),
        (Request::Connect { id: id() }, json!({ "op": "connect", "id": "c1" })),
        (Request::Display { id: id() }, json!({ "op": "display", "id": "c1" })),
        (Request::Version, json!({ "op": "version" })),
    ]
}

#[test]
fn every_request_is_the_json_magnus_sent_before_and_reads_back_as_itself() {
    for (req, old) in computerd_requests() {
        assert_eq!(serde_json::to_value(&req).unwrap(), old, "{req:?}");
        assert_eq!(serde_json::from_value::<Request>(old.clone()).unwrap(), req, "computerd misreads {old}");
        assert_eq!(old["op"], req.op(), "op() is not what goes on the wire");
    }
}

/// The exact line on the socket, for a representative few.
#[test]
fn the_lines_on_the_socket() {
    let line = |r: &Request| serde_json::to_string(r).unwrap();
    assert_eq!(line(&Request::Wake { id: "c1".into() }), r#"{"op":"wake","id":"c1"}"#);
    assert_eq!(line(&Request::Capacity), r#"{"op":"capacity"}"#);
    assert_eq!(line(&Request::Whose { uid: 7 }), r#"{"op":"whose","uid":7}"#);
    assert_eq!(line(&Request::Destroy { id: "c1".into(), delete_volume: true }), r#"{"op":"destroy","id":"c1","delete_volume":true}"#);
    assert_eq!(line(&Request::StartHelper { id: "c1".into(), read_only: false }), r#"{"op":"start_helper","id":"c1","read_only":false}"#);
    assert_eq!(line(&Request::SealVolume { id: "c1".into(), key_b64: "k".into() }), r#"{"op":"seal_volume","id":"c1","key_b64":"k"}"#);
    assert_eq!(line(&Request::Version), r#"{"op":"version"}"#);
}

#[test]
fn a_server_from_before_the_defaults_is_still_read() {
    // `read_only` and `delete_volume` were optional on the wire.
    assert_eq!(serde_json::from_str::<Request>(r#"{"op":"destroy","id":"c1"}"#).unwrap(), Request::Destroy { id: "c1".into(), delete_volume: false });
    assert_eq!(serde_json::from_str::<Request>(r#"{"op":"start_helper","id":"c1"}"#).unwrap(), Request::StartHelper { id: "c1".into(), read_only: false });
}

#[test]
fn an_unknown_request_or_field_is_refused() {
    for bad in [
        r#"{"op":"a_request_from_the_future","id":"c1"}"#,
        r#"{"op":"create","id":"a","path":"/etc/shadow"}"#,
        r#"{"op":"start","id":"a","memory_mib":99999}"#,
        r#"{"op":"run","command":"sh"}"#,
        r#"{"id":"c1"}"#,
    ] {
        assert!(serde_json::from_str::<Request>(bad).is_err(), "{bad} was accepted");
    }
}

/// serde names every variant when it refuses an unknown one.
fn every_variant() -> Vec<String> {
    let e = serde_json::from_str::<Request>(r#"{"op":"no_such_request"}"#).unwrap_err().to_string();
    let list = e.split("expected one of").nth(1).expect("serde lists the variants");
    list.split(',').map(|s| s.split('`').nth(1).expect("a quoted name").to_string()).collect()
}

#[test]
fn ops_names_every_request() {
    let mut ops: Vec<String> = OPS.iter().map(|s| s.to_string()).collect();
    let mut all = every_variant();
    ops.sort();
    all.sort();
    assert_eq!(ops, all);
    let mut tested: Vec<String> = computerd_requests().iter().map(|(r, _)| r.op().to_string()).collect();
    tested.sort();
    tested.dedup();
    assert_eq!(tested, all, "a request has no wire test");
}

#[test]
fn the_version_reply_is_read_leniently() {
    let v: VersionReply = serde_json::from_value(json!({ "ok": true, "version": "0.2.6", "base_id": null, "ops": ["version"] })).unwrap();
    assert_eq!(v, VersionReply { version: "0.2.6".into(), base_id: None, ops: vec!["version".into()] });
    assert_eq!(serde_json::from_value::<VersionReply>(json!({ "ok": true })).unwrap(), VersionReply::default());
    let sent = serde_json::to_value(VersionReply { version: "1".into(), base_id: Some("abc".into()), ops: vec!["capacity".into()] }).unwrap();
    assert_eq!(sent, json!({ "version": "1", "base_id": "abc", "ops": ["capacity"] }));
}

/// Each guest request beside the `json!` Magnus built for it.
fn guest_requests() -> Vec<(GuestRequest, Value)> {
    vec![
        (GuestRequest::Ping, json!({ "op": "ping" })),
        (GuestRequest::Sync, json!({ "op": "sync" })),
        (GuestRequest::ZeroFree, json!({ "op": "zero_free" })),
        (GuestRequest::TakeoverBegin, json!({ "op": "takeover_begin" })),
        (GuestRequest::TakeoverEnd, json!({ "op": "takeover_end" })),
        (GuestRequest::TakeoverLeftovers, json!({ "op": "takeover_leftovers" })),
        (GuestRequest::exec("update-ca-certificates", 60), json!({ "op": "exec", "command": "update-ca-certificates", "timeout_s": 60 })),
        (GuestRequest::read("/workspace/a"), json!({ "op": "read", "path": "/workspace/a" })),
        (
            GuestRequest::Read { path: "/workspace/a".into(), offset: Some(0), length: Some(1048576) },
            json!({ "op": "read", "path": "/workspace/a", "offset": 0, "length": 1048576 }),
        ),
        (GuestRequest::write("/workspace/a", "b25l"), json!({ "op": "write", "path": "/workspace/a", "data_b64": "b25l" })),
        (
            GuestRequest::Write { path: "/workspace/a".into(), data_b64: "b25l".into(), append: Some(true) },
            json!({ "op": "write", "path": "/workspace/a", "data_b64": "b25l", "append": true }),
        ),
        (GuestRequest::List { path: "/workspace/".into() }, json!({ "op": "list", "path": "/workspace/" })),
        (GuestRequest::Erase { path: "/workspace/a".into() }, json!({ "op": "erase", "path": "/workspace/a" })),
        (GuestRequest::Freeze, json!({ "op": "freeze" })),
        (GuestRequest::Thaw, json!({ "op": "thaw" })),
        (
            GuestRequest::Reset {
                unix_seconds: 1_794_000_000,
                machine_id: "0123456789abcdef0123456789abcdef".into(),
                hostname: "computer-0a1b2c".into(),
                mac: "06:00:ac:10:00:02".into(),
                entropy_b64: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
            },
            json!({
                "op": "reset",
                "unix_seconds": 1_794_000_000u64,
                "machine_id": "0123456789abcdef0123456789abcdef",
                "hostname": "computer-0a1b2c",
                "mac": "06:00:ac:10:00:02",
                "entropy_b64": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            }),
        ),
    ]
}

/// The lines above as they go on the wire, one per request in the same
/// order. The guest's tests read this file, so the guest is held to accept
/// every line a sender writes.
const GUEST_LINES: &str = include_str!("../guest_lines.jsonl");

#[test]
fn the_guest_lines_file_is_every_guest_request() {
    let lines: Vec<&str> = GUEST_LINES.lines().collect();
    let sent: Vec<String> = guest_requests().iter().map(|(r, _)| serde_json::to_string(r).unwrap()).collect();
    assert_eq!(lines, sent);
}

#[test]
fn every_guest_op_has_a_wire_test() {
    let mut tested: Vec<String> = guest_requests().iter().map(|(_, v)| v["op"].as_str().unwrap().to_string()).collect();
    tested.sort();
    tested.dedup();
    // serde names every op the guest knows when it refuses one it does not.
    let refused = serde_json::from_value::<GuestRequest>(json!({ "op": "a_request_from_the_future" })).unwrap_err().to_string();
    let list = refused.split("expected one of").nth(1).expect("serde lists the variants");
    let mut known: Vec<String> = list.split(',').map(|s| s.split('`').nth(1).expect("a quoted name").to_string()).collect();
    known.sort();
    assert_eq!(tested, known, "{refused}");
}

#[test]
fn the_guest_ignores_a_field_it_does_not_know_and_refuses_a_missing_one() {
    let newer = json!({ "op": "read", "path": "/workspace/a", "from_a_newer_server": 1 });
    assert_eq!(serde_json::from_value::<GuestRequest>(newer).unwrap(), GuestRequest::read("/workspace/a"));
    for missing in
        [json!({ "op": "read" }), json!({ "op": "write", "path": "/workspace/a" }), json!({ "op": "exec", "command": "ls" }), json!({ "op": "reset", "unix_seconds": 1 })]
    {
        assert!(serde_json::from_value::<GuestRequest>(missing.clone()).is_err(), "{missing}");
    }
}

#[test]
fn every_guest_request_is_the_json_magnus_sent_before() {
    for (req, old) in guest_requests() {
        assert_eq!(serde_json::to_value(&req).unwrap(), old, "{req:?}");
        assert_eq!(serde_json::from_value::<GuestRequest>(old.clone()).unwrap(), req, "{old}");
    }
    assert_eq!(serde_json::to_string(&GuestRequest::read("/w/a")).unwrap(), r#"{"op":"read","path":"/w/a"}"#, "an absent offset went on the wire");
    assert_eq!(serde_json::to_string(&GuestRequest::exec("ls", 5)).unwrap(), r#"{"op":"exec","command":"ls","timeout_s":5}"#);
}
