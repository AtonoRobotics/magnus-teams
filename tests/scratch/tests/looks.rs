//! The looks as the design draws them (owner, 2026-11-01: "themes as built
//! do not match the spec and references exactly"). The appliance is set up
//! with the scene of the Magnus Workspace references (a project's
//! conversation whose request became a finished trip, a supplier search
//! waiting on an approval, and a weekly schedule to confirm, with the trip
//! open beside it); `desktop/tests/looks.mjs` opens it in each look the
//! references draw (light solid, light glass, dark glass) and Settings >
//! Appearance, and checks every element's values against the references'.

use magnus_invariants::appliance::workspace_root;
use magnus_invariants::{mcp, *};
use magnus_scratch::*;
use serde_json::json;

const CHAT: &str = "talking with the person you work for in a chat";
const ASK: &str = "Plan a 3-day San Diego trip for two under $1,000. Find carbon fiber tube suppliers in SoCal that match my spec sheet. And every Monday, send me the latest on AI regulation in California.";
const TRIP: &str = "Plan the San Diego trip: three days, two people, under one thousand dollars.";
const SUPPLIERS: &str = "Search SoCal carbon fiber tube suppliers against the spec sheet.";

fn said(c: &Conversation) -> String {
    let first = c.messages().first().cloned().unwrap_or_default();
    first["content"].as_array().and_then(|b| b.last()).and_then(|b| b["text"].as_str()).unwrap_or_default().to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn each_look_draws_the_workspace_as_the_references_do() {
    let mail = mcp::mail().start().await;
    let mut m = FakeModel::start(script(|c| async move {
        let system = c.body["system"].to_string();
        if system.contains(CHAT) {
            return if said(&c).contains("carbon fiber tube suppliers") || c.body["messages"].to_string().contains("carbon fiber tube suppliers") {
                match c.turn() {
                    0 => tool("create_task", json!({ "title": "San Diego trip for two", "text": TRIP })),
                    1 => tool("create_task", json!({ "title": "Carbon fiber tube suppliers", "text": SUPPLIERS })),
                    2 => tool("create_schedule", json!({ "title": "AI regulation digest", "text": "Send me the latest on AI regulation in California.", "every_minutes": 10080, "scope": "Reads the web; sends you one message." })),
                    _ => Reply::Text("I've split that into two tasks and a weekly schedule. The trip is ready — it's open on the right. The supplier search needs one approval, and the schedule needs your OK.".into()),
                }
            } else {
                Reply::Text("Noted.".into())
            };
        }
        if c.body.to_string().contains(SUPPLIERS) {
            return match c.turn() {
                0 => contract_unchecked("find suppliers", "three suppliers that match"),
                _ => tool("mail__send", json!({ "to": "sales@tubes.example", "subject": "Quote", "body": "Please quote 40 tubes." })),
            };
        }
        if !c.body.to_string().contains(TRIP) {
            return Reply::Text(String::new());
        }
        match c.turn() {
            0 => contract_unchecked("plan the trip", "a 3-day plan under $1,000"),
            _ => tool(
                "task_complete",
                json!({
                    "summary": "3-day itinerary · $912 total",
                    "deliverable": [
                        { "type": "timeline", "events": [
                            { "when": "Day 1 · Sat", "label": "Arrive and settle in", "note": "Check in Little Italy · lunch at the waterfront · sunset at La Jolla Cove" },
                            { "when": "Day 2 · Sun", "label": "Balboa Park and the bay", "note": "Museums and gardens · harbor cruise · dinner in North Park" },
                            { "when": "Day 3 · Mon", "label": "Coronado and home", "note": "Beach morning · brunch on Orange Ave · drive back by 4 PM" }
                        ] },
                        { "type": "table", "columns": ["", ""], "rows": [["Hotel, 2 nights", "$480"], ["Food", "$260"], ["Activities", "$120"], ["Gas and parking", "$52"], ["Total", "$912"]] }
                    ]
                }),
            ),
        }
    }))
    .await;
    m.url = provider(&m).await;
    let app = Appliance::start(Spec::default().app("mail", &mail, "public", "private", &[("send", "irreversible")]), &m).await;
    let alice = app.api("alice");
    let (st, p) = alice.post("/v1/projects", json!({ "name": "Operations" })).await;
    assert_eq!(st.as_u16(), 201, "{p}");
    let (st, v) = alice.post("/v1/chat", json!({ "text": ASK, "project": p["id"] })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    alice.wait_settled(v["task_id"].as_str().unwrap()).await;
    // The trip is done: its review accepted.
    let trip = eventually("the trip task", PATIENCE, || async { alice.tasks().await["tasks"].as_array().unwrap().iter().find(|t| t["text"] == TRIP).cloned() }).await;
    let id = trip["task_id"].as_str().unwrap().to_string();
    let review = alice.wait_item(&id, "review").await;
    assert!(alice.respond(review["id"].as_str().unwrap(), json!({ "decision": "accept" })).await.is_success());
    assert_eq!(alice.wait_settled(&id).await["status"], "completed");
    let suppliers = eventually("the supplier search waits on its approval", PATIENCE, || async {
        alice.tasks().await["tasks"].as_array().unwrap().iter().find(|t| t["text"] == SUPPLIERS && t["waiting_on"] == "approval").cloned()
    })
    .await;
    assert!(suppliers["task_id"].is_string());

    assert!(set_password(&app, "alice", PASSWORD));
    let out = tokio::process::Command::new("node")
        .arg(workspace_root().join("desktop/tests/looks.mjs"))
        .env("MAGNUS_URL", &app.base)
        .env("MAGNUS_PERSON", "alice")
        .env("MAGNUS_PASSWORD", PASSWORD)
        .env("MAGNUS_TASK", &id)
        .env("MAGNUS_CHROMIUM", chromium())
        .envs(shots())
        .output()
        .await
        .unwrap();
    let log = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "the looks differ from the references:\n{log}");
    println!("{log}");
}

/// Each look is measured again until every part it checks is drawn, not
/// once after a fixed pause: a live update redrawing the task panel left
/// the light-solid look with none of its parts on run 42169676436 (#250).
#[test]
fn each_look_is_measured_once_every_part_is_drawn() {
    let src = std::fs::read_to_string(workspace_root().join("desktop/tests/looks.mjs")).unwrap();
    assert!(src.contains("while (missing(got) && Date.now() < until)"), "looks.mjs measures each look once, after a fixed pause");
}
