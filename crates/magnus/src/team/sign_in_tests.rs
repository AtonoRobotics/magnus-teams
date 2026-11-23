//! A workspace's own sign-in: kept under the workspace's key, used by its
//! team threads only, and never stood in for by a member's own sign-in.
//! sam owns the workspace, mo is a member.

use super::{is_workspace_owner, owner_key};
use crate::admin_app::tests::{server_with, Server};
use crate::run::admission::Refusal;
use axum::Router;
use serde_json::json;

/// One app people sign in to, one with no sign-in, one the workspace is not granted.
const APPS: &str =
    "[[connectors]]\nname = \"crm\"\nurl = \"https://crm.example/mcp\"\ncredential = \"connector_crm\"\nauth = \"oauth\"\naudience = \"internal\"\nlabel = \"private\"\n\
                    [[connectors]]\nname = \"open\"\nurl = \"https://open.example/mcp\"\naudience = \"internal\"\nlabel = \"untrusted\"\n\
                    [[connectors]]\nname = \"forge\"\nurl = \"https://forge.example/mcp\"\ncredential = \"connector_forge\"\nauth = \"oauth\"\naudience = \"internal\"\nlabel = \"private\"\n";

fn signed_in(s: &Server, app: &str, owner: &str) {
    let t = json!({ "access_token": "at", "refresh_token": "rt", "client": { "token_endpoint": "http://127.0.0.1:9/token", "client_id": "magnus", "resource": "r" } });
    s.k.vault.put(&crate::connectors::connection_name(app, owner), &t.to_string()).unwrap();
}

/// The workspace, with crm and open granted to every role, and a team run of mo's.
async fn team_run(s: &Server) -> (String, String, String, crate::state::Run) {
    let sam = s.token("sam").await;
    let (st, v) = s.post(&sam, "/v1/workspaces", json!({ "name": "Launch" })).await;
    assert_eq!(st, 200, "{v}");
    let ws = v["id"].as_str().unwrap().to_string();
    assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/members"), json!({ "person": "mo", "role": "member" })).await.0, 200);
    for app in ["crm", "open"] {
        assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/connectors"), json!({ "app": app, "roles": ["owner", "admin", "member"] })).await.0, 200);
    }
    let channel = s.k.read(|st| st.team.workspace(&ws).unwrap().channels.keys().next().unwrap().clone());
    let (task, _) = s.k.recorded_team_run(&ws, &channel, "mo", "update the crm").await.unwrap();
    let run = s.k.read(|st| st.runs.get(&st.tasks[&task].run_id).cloned().unwrap());
    (ws, sam, s.token("mo").await, run)
}

fn offered(s: &Server, run: &crate::state::Run) -> Vec<String> {
    let spec = |name: &str| magnus_adapter_proto::ToolSpec { name: name.into(), description: String::new(), input_schema: json!({}) };
    s.k.admitted(run, vec![spec("crm__update"), spec("open__search")]).into_iter().map(|t| t.name).collect()
}

/// A workspace's key can never be a person's: a person id has no `:`.
#[test]
fn a_workspace_key_is_never_a_persons() {
    let key = owner_key("w1");
    assert!(is_workspace_owner(&key));
    assert!(!crate::accounts::valid_person_id(&key), "a workspace's key passes as a person id");
    for person in ["sam", "mo.b", "a_b-c"] {
        assert!(!is_workspace_owner(person), "{person}");
    }
}

/// A team run on an app people sign in to acts through the workspace's
/// sign-in; on an app with no sign-in, as before, through its person.
#[tokio::test(flavor = "multi_thread")]
async fn a_team_run_acts_through_the_workspaces_sign_in() {
    let s = server_with(APPS, |_| Router::new()).await;
    let (ws, _, _, run) = team_run(&s).await;
    let crm = s.k.connector("crm").unwrap();
    let open = s.k.connector("open").unwrap();
    assert_eq!(s.k.app_owner(&run, &crm), owner_key(&ws));
    assert_eq!(s.k.app_owner(&run, &open), run.person);
    s.done().await;
}

/// Without the workspace's own sign-in, a granted app people sign in to is
/// refused, even when the requester has signed in to it themselves; with
/// it, it is offered and admitted.
#[tokio::test(flavor = "multi_thread")]
async fn only_the_workspaces_own_sign_in_admits_a_team_run() {
    let s = server_with(APPS, |_| Router::new()).await;
    let (ws, _, _, run) = team_run(&s).await;
    signed_in(&s, "crm", "mo");
    assert!(!offered(&s, &run).contains(&"crm__update".to_string()), "a member's own sign-in served a team run");
    let why = s.k.admits(&run, "crm__update");
    assert!(matches!(&why, Err(Refusal::Denied { why, .. }) if why.contains("own sign-in") && why.contains("People page")), "{why:?}");
    signed_in(&s, "crm", &owner_key(&ws));
    let now = offered(&s, &run);
    assert!(now.contains(&"crm__update".to_string()) && now.contains(&"open__search".to_string()), "{now:?}");
    assert_eq!(s.k.admits(&run, "crm__update"), Ok(()));
    s.done().await;
}

/// The app's tools, as it lists them through the workspace's sign-in,
/// reach the team run's tools; through only a member's own, they do not.
#[tokio::test(flavor = "multi_thread")]
async fn a_team_run_lists_the_apps_tools_through_the_workspaces_sign_in() {
    let config = APPS.replacen("label = \"private\"\n", "label = \"private\"\n[connectors.tools.search]\nrisk = \"read\"\n", 1);
    let s = server_with(&config, |_| Router::new()).await;
    let (ws, _, _, run) = team_run(&s).await;
    let listed = vec![crate::apps::AppTool { name: "search".into(), description: "Search the CRM.".into(), input_schema: json!({ "type": "object" }), annotations: json!({}) }];
    let fresh = |owner: &str| (crate::connectors::connection_name("crm", owner), (std::time::Instant::now(), listed.clone()));
    let names = |specs: Vec<magnus_adapter_proto::ToolSpec>| specs.into_iter().map(|t| t.name).collect::<Vec<_>>();
    signed_in(&s, "crm", "mo");
    s.k.live_tools.lock().await.extend([fresh("mo")]);
    assert!(!names(s.k.tool_catalog(&run, true).await).contains(&"crm__search".to_string()), "a member's own sign-in listed tools for a team run");
    signed_in(&s, "crm", &owner_key(&ws));
    s.k.live_tools.lock().await.extend([fresh(&owner_key(&ws))]);
    let tools = names(s.k.every_tool_for(&run).await);
    assert!(tools.contains(&"crm__search".to_string()), "{tools:?}");
    s.done().await;
}

/// One workspace's sign-in never serves another workspace's team run.
#[tokio::test(flavor = "multi_thread")]
async fn a_workspaces_sign_in_is_its_own() {
    let s = server_with(APPS, |_| Router::new()).await;
    let (_, _, _, run) = team_run(&s).await;
    signed_in(&s, "crm", &owner_key("someone-else"));
    assert!(s.k.admits(&run, "crm__update").is_err(), "another workspace's sign-in admitted this one's run");
    s.done().await;
}

/// The team agent is told each granted app, whether it works here and how
/// the team gets it working, and the server's other apps.
#[tokio::test(flavor = "multi_thread")]
async fn the_team_agent_knows_the_apps_and_how_to_get_them() {
    let s = server_with(APPS, |_| Router::new()).await;
    let (ws, _, _, _) = team_run(&s).await;
    let w = s.k.read(|st| st.team.workspace(&ws).cloned().unwrap());
    let text = s.k.workspace_apps(&ws, &w);
    assert!(text.contains("(crm)") && text.contains("Not connected for the workspace yet"), "{text}");
    assert!(text.contains("(open)") && text.contains("Works here."), "{text}");
    assert!(text.contains("Other apps on this server") && text.contains("(forge)"), "{text}");
    signed_in(&s, "crm", &owner_key(&ws));
    assert!(s.k.workspace_apps(&ws, &w).contains("Connected for the workspace"), "{}", s.k.workspace_apps(&ws, &w));
    let list = s.k.workspace_app_list(&w);
    let crm = list.as_array().unwrap().iter().find(|a| a["name"] == "crm").unwrap();
    assert_eq!((crm["signs_in"].as_bool(), crm["signed_in"].as_bool()), (Some(true), Some(true)), "{crm}");
    let forge = list.as_array().unwrap().iter().find(|a| a["name"] == "forge").unwrap();
    assert_eq!(forge["roles"], json!([]), "{forge}");
    s.done().await;
}

/// Only a Team Admin signs the workspace in, and only to a granted app
/// people sign in to.
#[tokio::test(flavor = "multi_thread")]
async fn only_a_team_admin_signs_a_granted_app_in() {
    let s = server_with(APPS, |_| Router::new()).await;
    let (ws, sam, mo, _) = team_run(&s).await;
    let body = json!({ "redirect_uri": "http://127.0.0.1:1/cb" });
    assert_eq!(s.post(&mo, &format!("/v1/workspaces/{ws}/connectors/crm/sign-in"), body.clone()).await.0, 403);
    assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/connectors/forge/sign-in"), body.clone()).await.0, 404, "an app not granted was signed in");
    assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/connectors/open/sign-in"), body).await.0, 404, "an app with no sign-in was signed in");
    s.done().await;
}

/// The admin console lists people who connected an app; a workspace's own
/// sign-in is not a person.
#[test]
fn a_workspaces_sign_in_is_not_listed_as_a_person() {
    let vault = vec![crate::connectors::connection_name("crm", "mo"), crate::connectors::connection_name("crm", &owner_key("w1"))];
    assert_eq!(crate::admin_app::connectors::connected(&vault, &Default::default(), "crm"), vec!["mo".to_string()]);
}
