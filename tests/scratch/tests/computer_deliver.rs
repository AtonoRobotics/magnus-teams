//! A file the agent made on its computer reaches the person (owner,
//! 2026-11-13). The agent ran Blender and FreeCAD on its persistent computer,
//! but `deliver_file` read only the run's sandbox. The CAD package stayed at
//! a path on the computer, and the person was told to fetch it themselves.
//! Needs a real `magnus-computerd` (KVM, the guest images, passwordless
//! sudo), as on Strix Beta.

use magnus_invariants::*;
use serde_json::json;

const MODEL: &str = "ISO-10303-21; HEADER; FILE_NAME('leg.step'); ENDSEC; END-ISO-10303-21;";

/// The agent writes a model on its computer and delivers it by its path
/// there, without saying where it is: the delivery looks where it was
/// working, and the person downloads the same bytes.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_made_on_the_computer_is_delivered_to_the_person() {
    let write = format!("mkdir -p /workspace/cad && printf '%s' \"{MODEL}\" > /workspace/cad/leg.step");
    let model = FakeModel::start(steps(vec![
        tool("computer_exec", json!({ "command": write })),
        tool("deliver_file", json!({ "path": "/workspace/cad/leg.step" })),
        tool("deliver_file", json!({ "path": "/workspace/cad/missing.step" })),
        complete("Delivered the leg model."),
    ]))
    .await;
    let computerd = Computerd::start();
    let app = Appliance::start(Spec::default().computer(&computerd.socket), &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Make the leg model and give it to me.").await;
    let t = alice.wait_settled(&id).await;
    assert_eq!(t["status"], "completed", "{t}");

    let written = &model.at_turn(1).expect("the run stopped after writing").last_results()[0];
    assert!(!written.is_error, "the command failed on the computer: {}", written.text);
    let delivered = &model.at_turn(2).expect("the run stopped after delivering").last_results()[0];
    assert!(!delivered.is_error && delivered.text.contains("from your computer"), "the file on the computer was not delivered: {}", delivered.text);
    let missing = &model.at_turn(3).expect("the run stopped").last_results()[0];
    assert!(missing.is_error && missing.text.contains("neither your sandbox nor your computer"), "a missing file was not said plainly: {}", missing.text);

    let files = t["outputs"].as_array().cloned().unwrap_or_default();
    let out = files.iter().find(|f| f["name"] == "leg.step").unwrap_or_else(|| panic!("the delivery is not on the task: {t}"));
    let (st, body) = alice.get(&format!("/v1/tasks/{id}/outputs/{}", out["id"].as_str().unwrap_or_default())).await;
    assert!(st.is_success(), "{body}");
    assert_eq!(body, json!(MODEL), "the person got different bytes from what was on the computer");
}
