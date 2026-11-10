use super::{retiring, Retired};
use crate::state::Item;
use serde_json::{json, Value};

fn item(kind: &str, details: Value) -> Item {
    Item {
        id: "i1".into(),
        kind: kind.into(),
        task: Some("t1".into()),
        run: Some("r1".into()),
        person: "alice".into(),
        summary: String::new(),
        details,
        created_at: chrono::Utc::now(),
        resolution: None,
    }
}

#[test]
fn only_open_items_of_retired_asks_retire() {
    assert_eq!(retiring(&item("review", json!({ "criteria": [{ "id": "c1" }, { "id": "c2" }] }))), Some(Retired::Completed(vec!["c1".into(), "c2".into()])));
    assert_eq!(retiring(&item("contract_change", json!({}))), Some(Retired::Refused));
    for rule in [json!("kernel:audience"), json!("kernel:web_lookups"), json!("kernel:local_change"), json!("unknown_outcome"), Value::Null] {
        assert_eq!(retiring(&item("approval", json!({ "rule": rule }))), Some(Retired::Withdrawn), "{rule}");
    }
    // The four asks stay, and so does everything that is not an ask.
    for rule in [crate::asks::OUTSIDE_REQUEST, crate::asks::NEW_DESTINATION, crate::asks::LOCAL_ASK_FIRST, crate::payments::RULE] {
        assert_eq!(retiring(&item("approval", json!({ "rule": rule }))), None, "{rule}");
    }
    for kind in ["question", "notice", "suggestion", "payment"] {
        assert_eq!(retiring(&item(kind, json!({}))), None, "{kind}");
    }
    // Decided already: nothing to retire.
    let mut done = item("review", json!({}));
    done.resolution = Some(("accept".into(), None, None, "person:alice".into()));
    assert_eq!(retiring(&done), None);
}
