//! Routines and follow-ups against a kernel booted on a fresh database:
//! each is its member's own personal Magnus at work, in a mission lane of
//! theirs, and never a team-agent run (2026-11-22-retire-team-agent-runs).
//! A routine that cannot start is recorded and its author told, and the
//! routines after it still start.

use super::chat::{AgentMode, Chat};
use super::routines::Firing;
use super::{Change, Message};
use crate::admin_app::tests::{server_with, Server};
use crate::events::Ev;
use axum::Router;
use serde_json::{json, Value};

fn routine(by: &str) -> super::routines::Routine {
    super::routines::Routine { channel: "c".into(), text: "Sum up the day.".into(), every_hours: 1, by: by.into(), ..Default::default() }
}

/// A workspace of `owner`'s with the channel "c".
async fn room(s: &Server, ws: &str, owner: &str) {
    s.k.emit("test", Ev::Team(Change::Created { workspace: ws.into(), name: ws.into(), owner: owner.into() })).await.unwrap();
    s.k.emit("test", Ev::Team(Change::Channel { workspace: ws.into(), channel: "c".into(), name: "general".into() })).await.unwrap();
}

/// The one lane of the one mission in channel "c", as its owner sees it,
/// and the task it started.
fn only_lane(s: &Server, ws: &str, owner: &str) -> (Value, String) {
    let missions = s.k.read(|st| st.missions.view_for_room(ws, "c", owner));
    assert_eq!(missions.len(), 1, "{missions:?}");
    let lanes = missions[0]["lanes"].as_array().cloned().unwrap_or_default();
    assert_eq!(lanes.len(), 1, "{lanes:?}");
    let mission = missions[0]["id"].as_str().unwrap_or_default().to_string();
    let task = s.k.read(|st| st.missions.mission(&mission).and_then(|m| m.lanes.values().next().and_then(|l| l.child_task.clone())));
    (lanes[0].clone(), task.unwrap_or_else(|| panic!("the lane started no task: {lanes:?}")))
}

/// The run of `task`: its member's own, in the background, and no team run.
fn own_background_run(s: &Server, task: &str, person: &str) -> crate::state::Run {
    assert!(s.k.team_of(task).is_none(), "a team-agent run was started");
    let run = s.k.read(|st| st.runs.get(&st.tasks[task].run_id).cloned()).unwrap();
    assert_eq!(run.person, person, "the work did not run as its member's own agent");
    assert!(s.k.run_kind(&run.id).is_background(), "the work could ask its member mid-run");
    run
}

/// ana may use no model, so her routine cannot start; mo's, in the next
/// workspace, starts all the same. Before, the first failure ended the
/// pass: nothing after it started, and nothing recorded it.
#[tokio::test(flavor = "multi_thread")]
async fn a_routine_that_cannot_start_is_told_and_the_rest_still_start() {
    let s = server_with("", |_| Router::new()).await;
    for (ws, by) in [("w1", "ana"), ("w2", "mo")] {
        room(&s, ws, by).await;
        s.k.emit("test", Ev::Team(Change::Routine { workspace: ws.into(), id: format!("r_{ws}"), routine: routine(by) })).await.unwrap();
    }
    let none = crate::spend::Change::ModelsAllowed { role: None, person: Some("ana".into()), models: Some(vec![]) };
    s.k.emit("test", Ev::Spend(none)).await.unwrap();
    s.k.run_due_routines().await.unwrap();
    let fired = |ws: &str| s.k.read(|st| st.team.workspace(ws).unwrap().routines[&format!("r_{ws}")].fired.clone());
    assert_eq!(fired("w1"), vec![Firing::Failed], "the failed firing was not recorded");
    assert!(matches!(fired("w2").as_slice(), [Firing::Started(_)]), "a routine after a failing one did not start: {:?}", fired("w2"));
    let told = s.k.read(|st| st.inbox.items().values().filter(|i| i.person == "ana" && i.details["reason"] == "routine_failed").count());
    assert_eq!(told, 1, "the routine's author was not told it could not start");
    s.done().await;
}

/// A routine fires as a member lane of its author's own agent in its
/// channel, whose answer reports there unasked. It once started a run of
/// the retired team agent, with the team's tools and the team's spend.
#[tokio::test(flavor = "multi_thread")]
async fn a_routine_fires_as_a_lane_of_its_authors_own_agent() {
    let s = server_with("", |_| Router::new()).await;
    room(&s, "w", "mo").await;
    s.k.emit("test", Ev::Team(Change::Routine { workspace: "w".into(), id: "r".into(), routine: routine("mo") })).await.unwrap();
    s.k.run_due_routines().await.unwrap();
    let fired = s.k.read(|st| st.team.workspace("w").unwrap().routines["r"].fired.clone());
    let [Firing::Started(task)] = fired.as_slice() else { panic!("the routine did not fire: {fired:?}") };
    own_background_run(&s, task, "mo");
    let (lane, lane_task) = only_lane(&s, "w", "mo");
    assert_eq!(&lane_task, task, "the routine recorded a task that is not its lane's");
    assert_eq!((lane["kind"].as_str(), lane["owner"].as_str()), (Some("member_agent"), Some("mo")), "{lane}");
    assert_eq!(lane["objective"], json!("Sum up the day."), "{lane}");
    assert_eq!(lane["auto_release"], json!(true), "the routine's report would wait for its author to release it: {lane}");
    s.done().await;
}

/// A question left a day in a channel that lets Magnus join is followed up
/// once by its asker's own agent, with their question alone, and the
/// answer waits in their Airlock, as when they only name Magnus.
#[tokio::test(flavor = "multi_thread")]
async fn a_quiet_question_is_followed_up_by_its_askers_own_agent() {
    let s = server_with("", |_| Router::new()).await;
    room(&s, "w", "mo").await;
    s.k.emit("test", Ev::Team(Change::Chat { workspace: "w".into(), chat: Chat::AgentMode { channel: "c".into(), mode: AgentMode::WhenHelpful } })).await.unwrap();
    let at = chrono::Utc::now() - chrono::Duration::hours(30);
    let message = Message { id: "q".into(), sender: "mo".into(), text: "Who has the launch deck?".into(), thread: None, at };
    s.k.emit("person:mo", Ev::Team(Change::Message { workspace: "w".into(), channel: "c".into(), message })).await.unwrap();
    s.k.follow_up_quiet_threads().await.unwrap();
    let (lane, task) = only_lane(&s, "w", "mo");
    own_background_run(&s, &task, "mo");
    assert_eq!((lane["kind"].as_str(), lane["owner"].as_str()), (Some("member_agent"), Some("mo")), "{lane}");
    assert!(lane["objective"].as_str().is_some_and(|o| o.contains("Who has the launch deck?")), "{lane}");
    assert_eq!(lane["auto_release"], json!(false), "a follow-up nobody asked for would post unreleased: {lane}");
    s.k.follow_up_quiet_threads().await.unwrap();
    assert_eq!(s.k.read(|st| st.missions.view_for_room("w", "c", "mo").len()), 1, "a quiet thread was followed up twice");
    s.done().await;
}
