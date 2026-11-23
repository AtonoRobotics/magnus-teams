//! Computers and the computer service agree; a kill switch stops what the
//! work was using; a computer that cannot start holds up no one.

use super::super::testing::{person, server};
use super::super::{Change, KillMode, Role, Status};
use crate::events::Ev;
use crate::halts::Scope;

/// A deletion made while the computer service is restarting is recorded at
/// once, and the service deletes the computer at the next look.
#[tokio::test(flavor = "multi_thread")]
async fn a_delete_while_the_service_is_down_finishes_at_the_next_look() {
    let s = server().await;
    let id = s.k.computer_for(&person("mo")).await.unwrap().unwrap();
    s.fake.set(|f| f.down = true);
    assert_eq!(s.k.delete_computers_of("mo").await.unwrap(), 1, "an erasure failed because the service was restarting");
    assert!(s.k.read(|st| st.computers.get(&id).is_none()), "the log still has the deleted computer");
    assert!(s.fake.held().contains(&id), "the service deleted it while it was down");
    let destroys = || s.asked("destroy").into_iter().filter(|r| r["id"] == id.as_str() && r["delete_volume"] == true).count();
    let while_down = destroys();
    s.fake.set(|f| f.down = false);
    s.k.agree_with_computerd().await;
    assert_eq!(destroys(), while_down + 1, "the service was not asked again to delete it");
    assert!(!s.fake.held().contains(&id), "the service still has the computer and its volume");
    s.k.agree_with_computerd().await;
    assert_eq!(destroys(), while_down + 1, "a computer already gone was deleted again");
    s.done().await;
}

/// A computer the log has (made before it was recorded, or by hand) is
/// never deleted by the look: only what the log deleted is.
#[tokio::test(flavor = "multi_thread")]
async fn the_look_deletes_only_what_the_log_deleted() {
    let s = server().await;
    let id = s.k.computer_for(&person("mo")).await.unwrap().unwrap();
    s.k.agree_with_computerd().await;
    assert!(s.asked("destroy").is_empty());
    assert!(s.fake.held().contains(&id));
    // An id the log deleted and then made again is the log's again.
    let killed = Change::Killed { id: id.clone(), mode: KillMode::Destroy, delete_volume: true };
    s.k.emit("kernel", Ev::Computer(killed)).await.unwrap();
    s.k.emit("kernel", Ev::Computer(Change::Created { id: id.clone(), owner: person("mo") })).await.unwrap();
    s.k.agree_with_computerd().await;
    assert!(s.fake.held().contains(&id), "a computer the log has was deleted");
    s.done().await;
}

/// A person's kill switch stops their browsers: a signed-in one is stopped
/// and its profile sealed, a throwaway one is deleted, so a run continued
/// later reads on a blank page.
#[tokio::test(flavor = "multi_thread")]
async fn a_kill_switch_stops_the_browser_computers() {
    let s = server().await;
    let mo = s.k.person("mo").unwrap();
    let signed_in = s.k.create_identity(&mo, "shop.example").await.unwrap()["id"].as_str().unwrap().to_string();
    s.k.emit("kernel", Ev::Computer(Change::Started { id: signed_in.clone() })).await.unwrap();
    let throwaway = super::super::service::new_id();
    let role = Role::Throwaway { run: "r-mo".into() };
    s.k.emit("kernel", Ev::Computer(Change::BrowserCreated { id: throwaway.clone(), owner: person("mo"), role })).await.unwrap();
    let samuel = s.k.person("sam").unwrap();
    let theirs = s.k.create_identity(&samuel, "bank.example").await.unwrap()["id"].as_str().unwrap().to_string();
    s.k.emit("kernel", Ev::Computer(Change::Started { id: theirs.clone() })).await.unwrap();

    s.k.scoped_switch("person:sam", "Mo", Scope::Person("mo".into()), true).await.unwrap();
    let asked = |op: &str, id: &str| s.asked(op).iter().any(|r| r["id"] == id);
    assert!(asked("stop", &signed_in), "the signed-in browser kept running");
    assert!(asked("seal_volume", &signed_in), "its profile was left open");
    assert_eq!(s.k.read(|st| st.computers.get(&signed_in).map(|c| c.status)), Some(Status::Stopped));
    assert!(s.k.read(|st| st.computers.get(&throwaway).is_none()), "the throwaway browser is still there");
    assert!(s.asked("destroy").iter().any(|r| r["id"] == throwaway.as_str() && r["delete_volume"] == true));
    assert!(!asked("stop", &theirs), "another person's browser was stopped");
    s.done().await;
}

/// A computer that cannot start leaves the queue, so the next one starts in
/// its turn; one its person paused while it waited starts again when asked.
#[tokio::test(flavor = "multi_thread")]
async fn a_computer_that_cannot_start_holds_up_no_one_behind_it() {
    let s = server().await;
    let socket = s.fake.socket.clone();
    let (a, b) = (s.made_by_hand(&person("mo")).await, s.made_by_hand(&person("sam")).await);
    for id in [&a, &b] {
        s.k.emit("kernel", Ev::Computer(Change::Queued { id: id.clone() })).await.unwrap();
    }
    s.fake.set(|f| {
        f.broken.insert(a.clone());
    });
    s.k.start_next_in_queue(&socket).await;
    assert_eq!(s.k.read(|st| st.computers.get(&a).map(|c| c.status)), Some(Status::Stopped), "the broken computer stayed at the head");
    s.k.start_next_in_queue(&socket).await;
    assert_eq!(s.k.read(|st| st.computers.get(&b).map(|c| c.status)), Some(Status::Running), "the computer behind it did not start");

    // Paused by its person while it waited: there is no VM to resume, so
    // Start wakes it instead of failing for ever.
    let mo = s.k.person("mo").unwrap();
    s.fake.set(|f| f.broken.clear());
    s.k.emit("kernel", Ev::Computer(Change::Queued { id: a.clone() })).await.unwrap();
    s.k.kill_computer(&mo, &a, KillMode::Pause, false).await.unwrap();
    assert_eq!(s.k.read(|st| st.computers.get(&a).map(|c| c.status)), Some(Status::Paused));
    s.k.start_computer(&mo, &a).await.unwrap();
    assert_eq!(s.k.read(|st| st.computers.get(&a).map(|c| c.status)), Some(Status::Running), "a computer paused while it waited could not start again");
    s.done().await;
}
