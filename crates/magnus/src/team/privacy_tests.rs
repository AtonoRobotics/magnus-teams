//! What a workspace keeps from whom (spec rev 142, Team): a project's
//! channels, messages and files are its people's; working notes are the
//! thread's and expire with the task; an answer built from the requester's
//! own data is theirs until they share it; and a team run never uses a
//! member's own sign-in. sam owns the workspace, mo is a member, ana a guest.

use crate::admin_app::tests::{server_with, Server};
use crate::events::Ev;
use crate::team::{Change, Note};
use axum::Router;
use serde_json::{json, Value};

/// Two granted apps: one each person signs in to, one with no sign-in.
const APPS: &str =
    "[[connectors]]\nname = \"crm\"\nurl = \"https://crm.example/mcp\"\ncredential = \"connector_crm\"\nauth = \"oauth\"\naudience = \"internal\"\nlabel = \"private\"\n\
                    [[connectors]]\nname = \"open\"\nurl = \"https://open.example/mcp\"\naudience = \"internal\"\nlabel = \"untrusted\"\n";

async fn workspace(s: &Server) -> (String, String, String, String) {
    let sam = s.token("sam").await;
    let (st, v) = s.post(&sam, "/v1/workspaces", json!({ "name": "Launch" })).await;
    assert_eq!(st, 200, "{v}");
    let ws = v["id"].as_str().unwrap().to_string();
    for (who, role) in [("mo", "member"), ("ana", "guest")] {
        assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/members"), json!({ "person": who, "role": role })).await.0, 200);
    }
    (ws, sam, s.token("mo").await, s.token("ana").await)
}

fn ids(v: &Value, key: &str, field: &str) -> Vec<String> {
    v[key].as_array().into_iter().flatten().filter_map(|c| c[field].as_str().map(String::from)).collect()
}

/// A project's channel, its messages, its notes and its files are open to
/// its people only, on every route, guests included.
#[tokio::test(flavor = "multi_thread")]
async fn a_projects_channel_messages_and_files_are_its_peoples_only() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, sam, mo, ana) = workspace(&s).await;
    let project = s.post(&sam, &format!("/v1/workspaces/{ws}/projects"), json!({ "name": "Secret" })).await.1["id"].as_str().unwrap().to_string();
    let channel = s.post(&sam, &format!("/v1/workspaces/{ws}/channels"), json!({ "name": "deal" })).await.1["id"].as_str().unwrap().to_string();
    assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/channels/{channel}/project"), json!({ "project": project })).await.0, 200);
    assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/channels/{channel}/messages"), json!({ "text": "the price is 4m" })).await.0, 200);
    let put = reqwest::Client::new().put(format!("{}/v1/workspaces/{ws}/files/terms.txt?project={project}", s.base)).bearer_auth(&sam).body("terms").send().await.unwrap();
    assert_eq!(put.status().as_u16(), 200);

    let (_, mine) = s.get(&sam, &format!("/v1/workspaces/{ws}")).await;
    assert!(ids(&mine, "channels", "id").contains(&channel) && ids(&mine, "files", "name").contains(&"terms.txt".to_string()), "{mine}");
    assert_eq!(s.get(&sam, &format!("/v1/workspaces/{ws}/channels/{channel}/messages")).await.0, 200);

    for (who, t) in [("mo", &mo), ("ana", &ana)] {
        let (_, v) = s.get(t, &format!("/v1/workspaces/{ws}")).await;
        assert!(!ids(&v, "channels", "id").contains(&channel), "{who} is shown a project channel they are not in: {v}");
        assert!(!ids(&v, "files", "name").contains(&"terms.txt".to_string()), "{who} is shown a project file they are not in: {v}");
        for path in ["messages", "view", "notes"] {
            assert_eq!(s.get(t, &format!("/v1/workspaces/{ws}/channels/{channel}/{path}")).await.0, 404, "{who} reads {path} of a project channel");
        }
        assert_eq!(s.post(t, &format!("/v1/workspaces/{ws}/channels/{channel}/messages"), json!({ "text": "hi" })).await.0, 404, "{who} posts in it");
        assert_eq!(s.get(t, &format!("/v1/workspaces/{ws}/files/terms.txt")).await.0, 404, "{who} reads a project file");
        let (_, panel) = s.get(t, &format!("/v1/workspaces/{ws}/panel")).await;
        assert!(!panel["files"].to_string().contains("terms.txt"), "{who} sees the file on the panel: {panel}");
    }
    s.done().await;
}

/// Working notes: seen only by those in the thread, and gone, content and
/// all, once their task ends.
#[tokio::test(flavor = "multi_thread")]
async fn working_notes_are_the_threads_and_expire_with_the_task() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, sam, mo, ana) = workspace(&s).await;
    let (_, d) = s.post(&sam, &format!("/v1/workspaces/{ws}/direct"), json!({ "people": ["mo"] })).await;
    let dm = d["id"].as_str().unwrap_or_else(|| panic!("{d}")).to_string();
    let run =
        Change::Run { workspace: ws.clone(), channel: dm.clone(), thread: None, task: "t1".into(), requester: "sam".into(), history: String::new(), sub: false, outside: false };
    s.k.emit("kernel", Ev::Team(run)).await.unwrap();
    let note = Note { id: "n1".into(), text: "the client may walk at 4m".into(), by: "agent:sam".into(), at: chrono::Utc::now() };
    s.k.emit("run:r", Ev::Team(Change::Note { workspace: ws.clone(), task: "t1".into(), note })).await.unwrap();
    assert_eq!(s.k.read(|st| st.team.notes("t1").len()), 1);
    assert_eq!(s.get(&ana, &format!("/v1/workspaces/{ws}/channels/{dm}/notes")).await.0, 404, "a guest outside the conversation read its notes");
    assert_eq!(s.get(&mo, &format!("/v1/workspaces/{ws}/channels/{dm}/notes")).await.0, 200);

    // The task is not running (it has no run at all here): its notes expire.
    s.k.expire_notes().await;
    assert!(s.k.read(|st| st.team.notes("t1").is_empty()), "an ended task kept its notes");
    let rows = s.k.log.of_kinds(&["team"], 100).await.unwrap();
    let seqs: Vec<i64> = rows.iter().filter(|(_, _, _, b)| b["change"] == "note").map(|(seq, _, _, _)| *seq).collect();
    assert_eq!(seqs.len(), 1);
    let kept = s.k.log.contents(&seqs).await.unwrap();
    assert!(!kept.values().any(|b| b.to_string().contains("walk at 4m")), "the note's content is still in the log: {kept:?}");
    let run_kept = rows.iter().any(|(_, _, _, b)| b["change"] == "run");
    assert!(run_kept, "the task's run record went with its notes");
    s.done().await;
}

/// A run that read the requester's own data answers them alone; the rest
/// of the thread is told only that it answered. The requester may share it.
#[tokio::test(flavor = "multi_thread")]
async fn a_private_answer_reaches_only_its_requester_until_shared() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, sam, mo, ana) = workspace(&s).await;
    let channel = s.k.read(|st| st.team.workspace(&ws).unwrap().channels.keys().next().unwrap().clone());
    let (task, _) = s.k.team_request(&ws, &channel, None, "mo", "what did my inbox say".into()).await.unwrap();
    let run = s.k.read(|st| st.runs.get(&st.tasks[&task].run_id).cloned().unwrap());
    let mut private = run.clone();
    private.private = true;
    let secret = "your inbox says the deal closes Friday".to_string();
    assert_eq!(super::member_view::answer_as(private.private, secret.clone(), "mo", "mo"), (secret.clone(), true));
    for viewer in ["sam", "ana"] {
        let (text, only) = super::member_view::answer_as(private.private, secret.clone(), "mo", viewer);
        assert!(only && !text.contains("Friday"), "{viewer} was shown a private answer: {text}");
    }
    assert_eq!(super::member_view::answer_as(run.private, "hello".into(), "mo", "ana"), ("hello".to_string(), false));
    // Sharing: only the requester, and only once it has finished.
    assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/answers/{task}/share"), json!({})).await.0, 404, "someone else shared mo's answer");
    assert_eq!(s.post(&ana, &format!("/v1/workspaces/{ws}/answers/{task}/share"), json!({})).await.0, 403);
    let (st, _) = s.post(&mo, &format!("/v1/workspaces/{ws}/answers/{task}/share"), json!({})).await;
    assert!(st == 409 || st == 200, "{st}");
    s.done().await;
}

/// A granted app that each person signs in to is neither offered to nor
/// usable by a team run: it would act through the requester's own sign-in.
/// An app with no sign-in, granted, is.
#[tokio::test(flavor = "multi_thread")]
async fn a_team_run_never_uses_a_members_own_sign_in() {
    let s = server_with(APPS, |_| Router::new()).await;
    let (ws, sam, _, _) = workspace(&s).await;
    for app in ["crm", "open"] {
        assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/connectors"), json!({ "app": app, "roles": ["owner", "admin", "member"] })).await.0, 200);
    }
    let channel = s.k.read(|st| st.team.workspace(&ws).unwrap().channels.keys().next().unwrap().clone());
    let (task, _) = s.k.team_request(&ws, &channel, None, "mo", "update the crm".into()).await.unwrap();
    let run = s.k.read(|st| st.runs.get(&st.tasks[&task].run_id).cloned().unwrap());
    let spec = |name: &str| magnus_adapter_proto::ToolSpec { name: name.into(), description: String::new(), input_schema: json!({}) };
    let offered: Vec<String> = s.k.team_scoped(&run, vec![spec("crm__update"), spec("open__search")]).into_iter().map(|t| t.name).collect();
    assert!(!offered.contains(&"crm__update".to_string()), "a member's own sign-in was offered to a team run: {offered:?}");
    assert!(offered.contains(&"open__search".to_string()), "{offered:?}");
    let action = |tool: &str| crate::boundary::Action {
        run: run.id.clone(),
        call_id: "c".into(),
        step_id: "s".into(),
        tool: tool.into(),
        input: json!({}),
        issuer: crate::boundary::Issuer::Model,
    };
    let why = s.k.team_refusal(&action("crm__update"), &run);
    assert!(why.as_deref().is_some_and(|w| w.contains("own sign-in")), "{why:?}");
    assert_eq!(s.k.team_refusal(&action("open__search"), &run), None);
    s.done().await;
}

/// A team's words reach a member's own runs only as private: a whisper's
/// run carries its thread, and a team run's summary listed in the member's
/// own conversation carries its workspace's words. Neither may be filed
/// for, or shown to, anyone outside the person.
#[tokio::test(flavor = "multi_thread")]
async fn a_teams_words_stay_private_in_a_members_own_runs() {
    let s = server_with("", |_| Router::new()).await;
    let (ws, sam, _, _) = workspace(&s).await;
    let channel = s.k.read(|st| st.team.workspace(&ws).unwrap().channels.keys().next().unwrap().clone());
    assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/channels/{channel}/messages"), json!({ "text": "the price is 4m" })).await.0, 200);
    for mine in [false, true] {
        let who = if mine { "sam" } else { "mo" };
        let (task, _) = s.k.whisper(&ws, &channel, None, who, "sum this up".into()).await.unwrap();
        let run = s.k.read(|st| st.runs.get(&st.tasks[&task].run_id).cloned().unwrap());
        assert!(run.private, "{who}'s whisper run is not private, so it could file the team's words for everyone");
    }
    let (task, _) = s.k.team_request(&ws, &channel, None, "mo", "what is the price".into()).await.unwrap();
    let (label, _) = s.k.listed_taint(&json!({ "tasks": [{ "id": task, "summary": "the price is 4m" }] }));
    assert!(label.private, "a team run's summary entered a member's conversation as public");
    s.done().await;
}

#[test]
fn an_expired_note_reads_as_nothing() {
    let mut l = crate::team::Ledger::default();
    l.apply(&Change::Note { workspace: "w".into(), task: "t".into(), note: Note::default() });
    assert!(l.notes("t").is_empty());
    l.apply(&Change::Note { workspace: "w".into(), task: "t".into(), note: Note { id: "n".into(), ..Default::default() } });
    l.drop_notes("t");
    assert!(l.notes("t").is_empty());
}
