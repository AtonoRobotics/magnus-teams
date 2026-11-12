//! The agreements: their text, the answers people give, and what those
//! answers let a developer view show.

use super::documents::{self, Versions};
use super::*;
use crate::admin_app::tests::{server_with, Server};
use crate::events::Ev;
use axum::Router;
use serde_json::{json, Value};

fn settings() -> Settings {
    Settings { operator: "Example Ltd".into(), contact: "magnus@example.com".into(), governing_law: "England and Wales".into(), tester_copy_days: 90 }
}

fn all_text(d: &documents::Doc) -> String {
    d.sections.iter().flat_map(|s| std::iter::once(s.heading.clone()).chain(s.paragraphs.clone())).collect::<Vec<_>>().join("\n")
}

#[test]
fn a_server_missing_a_legal_fact_does_not_start_and_says_which() {
    let e = Settings::default().check().err().map(|e| e.to_string()).unwrap_or_default();
    for name in ["operator", "contact", "governing_law", "tester_copy_days"] {
        assert!(e.contains(name), "{name} is not named: {e}");
    }
    let mut s = settings();
    s.contact = "nobody".into();
    assert!(s.check().is_err(), "an address with no @ was taken");
    assert!(settings().check().is_ok());
}

#[test]
fn the_terms_describe_teams_admins_and_privacy_as_the_owner_set_them() {
    let l = settings();
    let terms = all_text(&documents::terms(&l));
    for must in [
        "Anything posted or shared in a team workspace is visible to every member of that team",
        "owned by its Team Admin, who can moderate it and delete anything in it there",
        "You keep your own original",
        "Teams are isolated from each other",
        "you are a private party",
        "The Magnus admin does not see your data or any team's content",
        "only in a session you grant",
        "pausing a computer or stopping a task",
        "Example Ltd",
        "magnus@example.com",
        "England and Wales",
    ] {
        assert!(terms.contains(must), "the Terms of Use do not say {must:?}");
    }
    assert!(!terms.to_lowercase().contains("appliance"), "the terms say appliance");
    let testers = all_text(&documents::testers(&l));
    for must in [
        "in full",
        "conversations",
        "tool's inputs and results",
        "files your tasks touch",
        "screens",
        "Anonymous usage data is collected",
        "Example Ltd",
        "90 days",
        "Leave the program in Settings",
        "test software",
    ] {
        assert!(testers.contains(must), "the Tester Terms do not say {must:?}");
    }
    let privacy = all_text(&documents::privacy(&l));
    assert!(privacy.contains("If you say no, nothing of yours reaches them, not even anonymised"));
    for d in [documents::terms(&l), documents::privacy(&l)] {
        assert!(d.attribution.contains("basecamp/policies") && d.attribution.contains("Creative Commons Attribution 4.0"), "{} has no attribution", d.id);
    }
}

#[test]
fn a_changed_fact_changes_the_version_so_everyone_is_asked_again() {
    let a = Versions::of(&settings());
    let mut other = settings();
    other.operator = "Someone Else".into();
    let b = Versions::of(&other);
    assert_eq!(a, Versions::of(&settings()), "the same text gave another version");
    assert_ne!(a.terms, b.terms);
    assert_ne!(a.testers, b.testers);
}

#[test]
fn only_answers_to_the_text_as_it_reads_now_count() {
    let now = Versions::of(&settings());
    let mut l = Ledger::default();
    l.apply(&Change::UsageChosen { person: "ann".into(), share: true, version: now.usage.clone() });
    l.apply(&Change::UsageChosen { person: "old".into(), share: true, version: "2020-01-01.x".into() });
    l.apply(&Change::UsageChosen { person: "bob".into(), share: false, version: now.usage.clone() });
    l.apply(&Change::TesterJoined { person: "tess".into(), version: now.testers.clone() });
    l.apply(&Change::TesterJoined { person: "stale".into(), version: "2020-01-01.x".into() });
    assert_eq!(l.sharing_now(&now), ["ann".to_string()].into());
    assert_eq!(l.testers_now(&now), ["tess".to_string()].into());
    l.apply(&Change::TesterLeft { person: "tess".into(), version: now.testers.clone() });
    assert!(l.testers_now(&now).is_empty(), "leaving did not take effect");
}

const PEOPLE: &str = "[[people]]\nid = \"dev\"\nname = \"Dev\"\nroles = [\"developer\"]\n[[people]]\nid = \"ann\"\nname = \"Ann\"\n[[people]]\nid = \"bob\"\nname = \"Bob\"\n[[people]]\nid = \"tess\"\nname = \"Tess\"\n";

async fn server() -> Server {
    server_with(PEOPLE, |_| Router::new()).await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_person_accepts_the_terms_answers_the_prompt_and_joins_and_leaves_the_tester_program() {
    let s = server().await;
    let page = reqwest::get(format!("{}{}", s.base, documents::TERMS_PATH)).await.unwrap();
    assert_eq!(page.status().as_u16(), 200, "the terms are not public");
    assert!(page.text().await.unwrap().contains("Teams are isolated from each other"));

    let ann = s.token("ann").await;
    let (_, me) = s.get(&ann, "/v1/legal").await;
    assert_eq!((me["terms_due"].as_bool(), me["usage_due"].as_bool()), (Some(true), Some(true)), "{me}");
    let (st, _) = s.post(&ann, "/v1/legal/terms", json!({ "terms": "old", "privacy": "old" })).await;
    assert_eq!(st, 409, "terms accepted against a text nobody was shown");
    let docs = &me["documents"];
    let (st, me) = s.post(&ann, "/v1/legal/terms", json!({ "terms": docs["terms"]["version"], "privacy": docs["privacy"]["version"] })).await;
    assert_eq!((st, me["terms_due"].as_bool()), (200, Some(false)), "{me}");
    let (st, me) = s.post(&ann, "/v1/legal/usage", json!({ "share": false, "version": me["usage_prompt"]["version"] })).await;
    assert_eq!((st, me["usage"]["share"].as_bool(), me["usage_due"].as_bool()), (200, Some(false), Some(false)), "{me}");

    // Nobody joins by being put in the group: only by accepting the terms.
    assert!(!s.get(&ann, "/v1/me").await.1["groups"].as_array().unwrap().contains(&json!(TESTERS_GROUP)));
    let (st, _) = s.post(&ann, "/v1/legal/testers/join", json!({ "version": "old" })).await;
    assert_eq!(st, 409);
    let (st, me) = s.post(&ann, "/v1/legal/testers/join", json!({ "version": docs["testers"]["version"] })).await;
    assert_eq!((st, me["tester"]["current"].as_bool()), (200, Some(true)), "{me}");
    assert!(s.get(&ann, "/v1/me").await.1["groups"].as_array().unwrap().contains(&json!(TESTERS_GROUP)), "a tester is not in the testers group");

    // Admins see who is a tester, and may invite; members may not.
    let mo = s.token("mo").await;
    assert_eq!(s.get(&mo, "/v1/admin/testers").await.0, 403);
    let sam = s.token("sam").await;
    let (st, list) = s.get(&sam, "/v1/admin/testers").await;
    assert_eq!((st, &list["testers"][0]["person"]), (200, &json!("ann")), "{list}");

    let (st, me) = s.post(&ann, "/v1/legal/testers/leave", json!({})).await;
    assert_eq!((st, &me["tester"]), (200, &Value::Null));
    assert!(!s.get(&ann, "/v1/me").await.1["groups"].as_array().unwrap().contains(&json!(TESTERS_GROUP)), "leaving kept them in the group");
    let kinds: Vec<String> = s.k.read(|st| st.legal.usage_of("ann").map(|u| vec![u.1.clone()]).unwrap_or_default());
    assert_eq!(kinds.len(), 1, "the choice was not recorded with its version");
    s.done().await;
}

/// Answer for `who` as a person would.
async fn answer(s: &Server, who: &str, share: Option<bool>, tester: bool) {
    let t = s.token(who).await;
    let (_, me) = s.get(&t, "/v1/legal").await;
    if let Some(share) = share {
        assert_eq!(s.post(&t, "/v1/legal/usage", json!({ "share": share, "version": me["usage_prompt"]["version"] })).await.0, 200);
    }
    if tester {
        assert_eq!(s.post(&t, "/v1/legal/testers/join", json!({ "version": me["documents"]["testers"]["version"] })).await.0, 200);
    }
}

/// A run of `who`'s with words, an address and a URL only they wrote.
async fn run_of(s: &Server, who: &str) {
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
    let actor = format!("person:{who}");
    let evs = [
        json!({ "kind": "task_created", "task": format!("task-{who}"), "run": format!("run-{who}"), "person": who, "text": format!("SECRET-{who} email {who}@example.com about the merger"), "config": config }),
        json!({ "kind": "model_reply", "run": format!("run-{who}"), "step": 1, "blocks": [{ "type": "text", "text": format!("REPLY-{who}") }], "usage": { "input_tokens": 10, "output_tokens": 5, "cost_usd": 0.01 }, "served_model": "m", "stop": "tool_use" }),
        json!({ "kind": "intent", "run": format!("run-{who}"), "call_id": format!("c-{who}"), "step_id": "s1", "key": "k", "tool": "fetch", "input": { "url": format!("https://example.com/private/{who}?q=INPUT-{who}") }, "issuer": "model" }),
        json!({ "kind": "notice", "run": format!("run-{who}"), "step": 1, "text": format!("NOTICE-{who} file report-{who}.xlsx") }),
    ];
    for e in evs {
        let ev: Ev = serde_json::from_value(e.clone()).unwrap_or_else(|x| panic!("{e}: {x}"));
        s.k.emit(&actor, ev).await.unwrap();
    }
}

/// What a developer sees of ann (anonymous usage data), bob (said no) and
/// tess (a tester) on every developer route.
#[tokio::test(flavor = "multi_thread")]
async fn developer_views_show_testers_in_full_sharers_anonymised_and_nobody_else() {
    let s = server().await;
    answer(&s, "ann", Some(true), false).await;
    answer(&s, "bob", Some(false), false).await;
    answer(&s, "tess", None, true).await;
    for who in ["ann", "bob", "tess", "mo"] {
        run_of(&s, who).await;
    }
    // The developer views recall task words through the content walls, as
    // `magnus migrate` sets them up.
    let db = crate::log::connect(&s.k.cfg.database.url).await.unwrap();
    let role = url::Url::parse(&s.k.cfg.database.url).unwrap().username().to_string();
    db.batch_execute(&crate::log::content_walls_sql(&role)).await.unwrap();
    let dev = s.token("dev").await;
    let mut seen = String::new();
    for path in [
        "/v1/dev/runs",
        "/v1/dev/runs/run-ann",
        "/v1/dev/runs/run-ann/log",
        "/v1/dev/metrics",
        "/v1/dev/experiments",
        "/v1/dev/policy",
        "/v1/dev/playbook",
        "/v1/dev/modules",
        "/v1/dev/computers",
        "/v1/dev/tool-screens",
    ] {
        let (st, v) = s.get(&dev, path).await;
        assert_eq!(st, 200, "{path}: {v}");
        seen.push_str(&v.to_string());
    }
    // ann's words, address, URL path and query, file name and ids never appear.
    for leak in ["SECRET-ann", "ann@example.com", "REPLY-ann", "/private/ann", "INPUT-ann", "NOTICE-ann", "report-ann", "\"ann\"", "person:ann\"", "task-ann"] {
        assert!(!seen.contains(leak), "a developer saw {leak}");
    }
    // bob, who said no, and mo, never asked, are not there at all.
    for who in ["bob", "mo"] {
        for leak in [format!("SECRET-{who}"), format!("run-{who}"), format!("REPLY-{who}"), format!("\"{who}\""), format!("{who}.example")] {
            assert!(!seen.contains(&leak), "a developer saw {leak} of {who}'s");
        }
        for path in [format!("/v1/dev/runs/run-{who}"), format!("/v1/dev/runs/run-{who}/log")] {
            assert_eq!(s.get(&dev, &path).await.0, 404, "{path} was shown");
        }
    }
    // ann's run is there, with what finds bugs.
    let (_, runs) = s.get(&dev, "/v1/dev/runs").await;
    let ann = runs["runs"].as_array().unwrap().iter().find(|r| r["run"] == "run-ann").cloned().unwrap_or_default();
    assert_eq!((ann["status"].as_str(), ann["config"]["model"].as_str()), (Some("running"), Some("m")), "{ann}");
    assert!(ann["person"].as_str().is_some_and(|p| p.starts_with("anon-")), "{ann}");
    let (_, log) = s.get(&dev, "/v1/dev/runs/run-ann/log").await;
    let kinds: Vec<&str> = log["events"].as_array().unwrap().iter().filter_map(|e| e["kind"].as_str()).collect();
    assert!(kinds.contains(&"intent") && kinds.contains(&"model_reply"), "{log}");
    assert!(log.to_string().contains("\"fetch\""), "the tool's name was withheld: {log}");
    // tess, a tester, is shown in full.
    let (_, tess) = s.get(&dev, "/v1/dev/runs/run-tess/log").await;
    for full in ["SECRET-tess", "/private/tess", "REPLY-tess"] {
        assert!(tess.to_string().contains(full), "a tester's {full} was withheld: {tess}");
    }
    // Leaving the program applies from then on.
    let t = s.token("tess").await;
    assert_eq!(s.post(&t, "/v1/legal/testers/leave", json!({})).await.0, 200);
    assert_eq!(s.get(&dev, "/v1/dev/runs/run-tess/log").await.0, 404, "a former tester who never shared is still shown");
    s.done().await;
}
