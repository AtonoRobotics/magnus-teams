//! The outcome page shows what a task produced, not only that it ran. The
//! deliverable comes first, in full, drawn from the fixed components, with
//! delivered files shown inline; then the summary, the evidence, the cost
//! and the trace (`desktop/tests/outcome.mjs`). Found on the San Diego trip
//! (2026-10-28): its itinerary was only in a delivered file, and the page
//! showed a price, a chart and three check lines.

use magnus_invariants::appliance::workspace_root;
use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::json;

const ITINERARY: &str = "# San Diego for two, under $1,000\n\n## Day 1: Balboa Park\n\n- 9:00 Drive from Burbank, about 3 hours ($34 fuel)\n- 13:00 Check in at Mission View Inn, 641 Camino del Rio S ($67 a night)\n- 15:00 San Diego Natural History Museum ($45 for two)\n\n## Day 2: The harbor\n\n- 10:00 Embarcadero walk (free)\n- 12:30 Lunch at Tacos El Gordo ($28)\n\n## Day 3: La Jolla Cove\n\n- 8:30 Sea lions at La Jolla Cove (free)\n- 14:00 Drive home ($34 fuel)\n\n| Category | Cost |\n|---|---|\n| Transport | $174 |\n| Lodging | $155 |\n| Total | $914 |\n";
const COSTS: &str = "category,cost\ntransport,174\nlodging,155\nfood,240\n";

fn trip() -> Vec<Reply> {
    vec![
        tool(
            "task_contract",
            json!({ "title": "San Diego trip", "goal": "a 3-day itinerary under $1,000", "assumptions": [], "irreversible_actions": [],
            "criteria": [{ "id": "days", "description": "Three days are planned", "check": { "tool": "sandbox_exec", "input": { "command": "grep -c '^## Day' /workspace/itinerary.md" }, "expect": [{ "pointer": "/stdout", "contains": "3" }] } }] }),
        ),
        tool("sandbox_write_file", json!({ "path": "itinerary.md", "content": ITINERARY })),
        tool("deliver_file", json!({ "path": "itinerary.md", "name": "San Diego itinerary.md" })),
        tool("sandbox_write_file", json!({ "path": "costs.csv", "content": COSTS })),
        tool("deliver_file", json!({ "path": "costs.csv" })),
        // A file component naming a file never delivered is refused.
        tool("task_complete", json!({ "summary": "Planned.", "deliverable": [{ "type": "file", "file": "nowhere.md" }] })),
        tool(
            "task_complete",
            json!({
                "summary": "A 3-day plan for two at $914, driving from Burbank. Nothing was booked.",
                "deliverable": [{ "type": "stat", "label": "Estimated total", "value": "$914" }, { "type": "file", "file": "San Diego itinerary.md" }]
            }),
        ),
    ]
}

#[tokio::test(flavor = "multi_thread")]
async fn the_outcome_page_leads_with_the_full_deliverable() {
    let mut m = FakeModel::start(steps(trip())).await;
    m.url = provider(&m).await;
    let app = Appliance::start(Spec::default(), &m).await;
    assert!(set_password(&app, PERSON, PASSWORD));
    let alice = app.api(PERSON);
    let id = alice.submit("Plan a 3-day trip to San Diego for two under $1,000 and deliver an itinerary.").await;
    let t = alice.wait_status(&id, &["completed", "failed"]).await;
    assert_eq!(t["status"], "completed", "{t}");
    let refused = m.requests().into_iter().find(|c| c.turn() == 6).unwrap().last_results();
    assert!(refused.iter().any(|r| r.is_error && r.text.contains("not a file you delivered")), "{refused:?}");
    // The model is told to complete with the work itself.
    let spec = m.requests()[0].body["tools"].as_array().unwrap().iter().find(|t| t["name"] == "task_complete").cloned().unwrap();
    assert!(spec["input_schema"]["required"].as_array().unwrap().contains(&json!("deliverable")), "{spec}");
    assert!(spec.to_string().contains("each day with its places, times and costs"), "{spec}");

    let out = tokio::process::Command::new("node")
        .arg(workspace_root().join("desktop/tests/outcome.mjs"))
        .env("MAGNUS_URL", &app.base)
        .env("MAGNUS_PERSON", PERSON)
        .env("MAGNUS_PASSWORD", PASSWORD)
        .env("MAGNUS_TASK", &id)
        .env("MAGNUS_CHROMIUM", chromium())
        .envs(shots())
        .output()
        .await
        .unwrap();
    let log = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "the outcome check failed:\n{log}");
    // The verdict tapped on the page is in the log.
    let t = alice.get(&format!("/v1/tasks/{id}")).await.1;
    assert_eq!(t["verdict"]["verdict"], "partly", "{t}");
    assert_eq!(t["verdict"]["note"], "Lodging is far from the coast.", "{t}");
}

/// A completion must carry a deliverable: one that is missing, an empty list,
/// or only blank text is refused, and the task goes on.
/// Its shape is checked, not its content.
#[tokio::test(flavor = "multi_thread")]
async fn a_completion_with_an_empty_deliverable_is_refused() {
    let contract = tool(
        "task_contract",
        json!({ "goal": "close the ticket", "assumptions": [], "irreversible_actions": [],
        "criteria": [{ "id": "c1", "description": "the work is done", "check": { "tool": "sandbox_exec", "input": { "command": "true" }, "expect": [{ "pointer": "/exit_code", "equals": 0 }] } }] }),
    );
    let mut m = FakeModel::start(steps(vec![
        contract,
        tool("task_complete", json!({ "summary": "Closed the ticket." })),
        tool("task_complete", json!({ "summary": "Closed the ticket.", "deliverable": [] })),
        tool("task_complete", json!({ "summary": "Closed the ticket.", "deliverable": [{ "type": "text", "text": "  " }] })),
        tool("task_complete", json!({ "summary": "Closed the ticket.", "deliverable": [{ "type": "text", "text": "Closed ticket 42 with the note \"fixed in 1.2\"." }] })),
    ]))
    .await;
    m.url = provider(&m).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let alice = app.api(PERSON);
    let id = alice.submit("Close ticket 42.").await;
    let t = alice.wait_status(&id, &["completed", "failed"]).await;
    assert_eq!(t["status"], "completed", "{t}");
    let reqs = m.requests();
    for turn in [2, 3, 4] {
        let r = reqs.iter().find(|c| c.turn() == turn).unwrap().last_results();
        assert!(r.iter().any(|r| r.is_error && r.text.contains("The deliverable was not accepted")), "turn {turn}: {r:?}");
    }
    assert_eq!(t["outcome_view"][0]["text"], "Closed ticket 42 with the note \"fixed in 1.2\".", "{t}");
}
