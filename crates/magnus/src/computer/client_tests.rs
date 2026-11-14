use super::*;
use crate::computer::fake_computerd::{Fake, Turn};
use serde_json::json;

fn fake(turns: &[Turn]) -> Fake {
    let f = Fake::start();
    f.set(|s| s.turns = turns.iter().copied().collect());
    f
}

/// The guest's lines: every line after each `connect`.
fn guest_lines(f: &Fake) -> Vec<Value> {
    f.asked().into_iter().filter(|r| r["op"] != "connect").collect()
}

#[tokio::test]
async fn a_connection_that_never_reached_the_guest_is_opened_again() {
    let f = fake(&[Turn::Unreached, Turn::Unreached, Turn::Answers]);
    let reply = guest(&f.socket, "c1", GuestRequest::write("/workspace/a", "b25l")).await.unwrap();
    assert_eq!(reply["path"], "/workspace/a");
    assert_eq!(f.connections(), 3);
    assert_eq!(guest_lines(&f).len(), 1, "the request went more than once");
}

#[tokio::test]
async fn a_guest_never_reached_is_tried_a_bounded_number_of_times() {
    let f = fake(&[Turn::Unreached; GUEST_CONNECT_TRIES as usize + 1]);
    let e = guest(&f.socket, "c1", GuestRequest::Ping).await.unwrap_err();
    assert!(e.to_string().contains("reset by peer"), "{e:#}");
    assert_eq!(f.connections(), GUEST_CONNECT_TRIES as usize);
    assert!(guest_lines(&f).is_empty());
}

#[tokio::test]
async fn a_request_once_sent_is_never_sent_again() {
    let f = fake(&[Turn::TakesAndCloses, Turn::Answers]);
    let e = guest(&f.socket, "c1", GuestRequest::Write { path: "/workspace/a".into(), data_b64: String::new(), append: Some(true) }).await.unwrap_err();
    assert!(e.to_string().contains("closed the connection"), "{e:#}");
    assert_eq!(f.connections(), 1, "a request the guest may have run was sent again");
    assert_eq!(guest_lines(&f).len(), 1);
}

/// The lines on the socket are the typed requests, a JSON line each.
#[tokio::test]
async fn computerd_and_the_guest_are_sent_the_typed_requests() {
    let f = Fake::start();
    ask(&f.socket, Request::Wake { id: "c1".into() }).await.unwrap();
    guest(&f.socket, "c1", GuestRequest::exec("ls", 5)).await.unwrap();
    assert_eq!(f.asked(), [json!({ "op": "wake", "id": "c1" }), json!({ "op": "connect", "id": "c1" }), json!({ "op": "exec", "command": "ls", "timeout_s": 5 })]);
}

#[tokio::test]
async fn computerd_s_refusal_comes_back_in_its_words() {
    let f = Fake::start();
    f.set(|s| s.full = true);
    let e = ask(&f.socket, Request::Wake { id: "c1".into() }).await.unwrap_err();
    assert!(at_capacity(&e), "{e:#}");
}
