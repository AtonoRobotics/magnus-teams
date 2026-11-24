//! Invariant 3 for a co-edited file of a shared project: it changes only by
//! versions kept in the log, so its history is whole, a stale change that
//! conflicts is rejected with nothing lost, and a checked-out drawing has
//! one editor (the owner's design, Co-editing shared files; the kernel's
//! own tests are `crates/magnus/src/team/coedit/tests.rs`).

// Test code: a failed unwrap is a failed test (AGENTS.md: none outside tests).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use magnus_invariants::*;
use serde_json::{json, Value};

const SOW: &str = "Scope: the east wing.\n\nPrice: 40,000.\n\nStart: March.\n";

async fn team(alice: &Api) -> String {
    let (code, made) = alice.post("/v1/workspaces", json!({ "name": "Fit-out" })).await;
    assert_eq!(code, 200, "{made}");
    let ws = made["id"].as_str().unwrap().to_string();
    let (code, v) = alice.post(&format!("/v1/workspaces/{ws}/members"), json!({ "person": "bob", "role": "member" })).await;
    assert_eq!(code, 200, "{v}");
    ws
}

/// A file's bytes, as the app uploads them.
async fn put(who: &Api, path: &str, bytes: &[u8]) -> (reqwest::StatusCode, Value) {
    let r = reqwest::Client::new().put(format!("{}{path}", who.base)).bearer_auth(&who.token).body(bytes.to_vec()).send().await.unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

async fn version(who: &Api, ws: &str, name: &str, n: u32) -> Vec<u8> {
    let r = reqwest::Client::new().get(format!("{}/v1/workspaces/{ws}/coedit/{name}/versions/{n}", who.base)).bearer_auth(&who.token).send().await.unwrap();
    assert!(r.status().is_success(), "version {n} of {name} is gone");
    r.bytes().await.unwrap().to_vec()
}

async fn text(who: &Api, ws: &str, name: &str, n: u32) -> String {
    String::from_utf8(version(who, ws, name, n).await).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn changes_to_different_paragraphs_from_one_base_all_land_and_every_version_stays() {
    let model = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    let ws = team(&alice).await;
    assert_eq!(put(&alice, &format!("/v1/workspaces/{ws}/files/sow.md?share=coedit"), SOW.as_bytes()).await.0, 200);
    assert_eq!(put(&alice, &format!("/v1/workspaces/{ws}/files/sow.md?base=1"), SOW.replace("March", "April").as_bytes()).await.0, 200);
    let (code, v) = put(&bob, &format!("/v1/workspaces/{ws}/files/sow.md?base=1"), SOW.replace("40,000", "42,500").as_bytes()).await;
    assert_eq!((code.as_u16(), v["version"].clone()), (200, json!(3)), "{v}");
    assert_eq!(text(&bob, &ws, "sow.md", 3).await, "Scope: the east wing.\n\nPrice: 42,500.\n\nStart: April.\n");
    assert_eq!(text(&bob, &ws, "sow.md", 1).await, SOW, "the history is not whole");
    let (_, h) = bob.get(&format!("/v1/workspaces/{ws}/coedit/sow.md")).await;
    let bases: Vec<(u64, u64)> = h["versions"].as_array().unwrap().iter().map(|v| (v["number"].as_u64().unwrap(), v["base"].as_u64().unwrap())).collect();
    assert_eq!(bases, vec![(3, 1), (2, 1), (1, 0)], "a version lost its base: {h}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stale_change_to_the_same_paragraph_is_rejected_and_both_texts_survive() {
    let model = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    let ws = team(&alice).await;
    put(&alice, &format!("/v1/workspaces/{ws}/files/sow.md?share=coedit"), SOW.as_bytes()).await;
    let first = canary("price-first");
    let later = canary("price-later");
    assert_eq!(put(&alice, &format!("/v1/workspaces/{ws}/files/sow.md?base=1"), SOW.replace("40,000", &first).as_bytes()).await.0, 200);
    let (code, v) = put(&bob, &format!("/v1/workspaces/{ws}/files/sow.md?base=1"), SOW.replace("40,000", &later).as_bytes()).await;
    assert_eq!(code.as_u16(), 409, "a stale change overwrote a landed one: {v}");
    assert!(v.to_string().contains(&first) && v.to_string().contains(&later), "both versions of the paragraph are not shown: {v}");
    assert!(text(&alice, &ws, "sow.md", 2).await.contains(&first), "the landed text was lost");
    let (_, open) = bob.get(&format!("/v1/workspaces/{ws}/coedit-changes")).await;
    assert!(open.to_string().contains(&later), "the rejected text was lost: {open}");
    let (_, alices) = alice.get(&format!("/v1/workspaces/{ws}/coedit-changes")).await;
    assert!(!alices.to_string().contains(&later), "another member read the rejected change");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_checked_out_drawing_refuses_anothers_write_until_it_is_checked_in() {
    let model = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    let ws = team(&alice).await;
    assert_eq!(put(&alice, &format!("/v1/workspaces/{ws}/files/bracket.dwg?share=coedit"), &[0, 1, 2, 255]).await.0, 200);
    assert_eq!(alice.post(&format!("/v1/workspaces/{ws}/coedit/bracket.dwg/checkout"), json!({})).await.0, 200);
    assert_eq!(put(&bob, &format!("/v1/workspaces/{ws}/files/bracket.dwg?base=1"), &[9]).await.0.as_u16(), 409, "a checked-out drawing took another's write");
    assert_eq!(version(&bob, &ws, "bracket.dwg", 1).await, vec![0, 1, 2, 255], "others do not read the last version");
    assert_eq!(alice.post(&format!("/v1/workspaces/{ws}/coedit/bracket.dwg/checkin"), json!({})).await.0, 200);
    assert_eq!(put(&bob, &format!("/v1/workspaces/{ws}/files/bracket.dwg?base=1"), &[9]).await.0.as_u16(), 200);
}
