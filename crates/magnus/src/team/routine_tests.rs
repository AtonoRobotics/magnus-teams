//! The routine sweep against a kernel booted on a fresh database: a routine
//! that cannot start is recorded and its admin told, and the routines after
//! it still start.

use super::routines::Firing;
use super::Change;
use crate::admin_app::tests::server_with;
use crate::events::Ev;
use axum::Router;

fn routine(by: &str) -> super::routines::Routine {
    super::routines::Routine { channel: "c".into(), text: "Sum up the day.".into(), every_hours: 1, by: by.into(), ..Default::default() }
}

/// ana may use no model, so her routine cannot start; mo's, in the next
/// workspace, starts all the same. Before, the first failure ended the
/// pass: nothing after it started, and nothing recorded it.
#[tokio::test(flavor = "multi_thread")]
async fn a_routine_that_cannot_start_is_told_and_the_rest_still_start() {
    let s = server_with("", |_| Router::new()).await;
    for (ws, by) in [("w1", "ana"), ("w2", "mo")] {
        s.k.emit("test", Ev::Team(Change::Created { workspace: ws.into(), name: ws.into(), owner: by.into() })).await.unwrap();
        s.k.emit("test", Ev::Team(Change::Routine { workspace: ws.into(), id: format!("r_{ws}"), routine: routine(by) })).await.unwrap();
    }
    let none = crate::spend::Change::ModelsAllowed { role: None, person: Some("ana".into()), models: Some(vec![]) };
    s.k.emit("test", Ev::Spend(none)).await.unwrap();
    s.k.run_due_routines().await.unwrap();
    let fired = |ws: &str| s.k.read(|st| st.team.workspace(ws).unwrap().routines[&format!("r_{ws}")].fired.clone());
    assert_eq!(fired("w1"), vec![Firing::Failed], "the failed firing was not recorded");
    assert!(matches!(fired("w2").as_slice(), [Firing::Started(_)]), "a routine after a failing one did not start: {:?}", fired("w2"));
    let told = s.k.read(|st| st.items.values().filter(|i| i.person == "ana" && i.details["reason"] == "routine_failed").count());
    assert_eq!(told, 1, "the routine's admin was not told it could not start");
    s.done().await;
}
