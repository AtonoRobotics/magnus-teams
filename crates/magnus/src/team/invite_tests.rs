//! Inviting by email or link (spec rev 142, Team shared workspace and
//! Administration, People), against a kernel on a fresh database: a known
//! address joins at once; an unknown one gets an account invitation by
//! email and joins when they choose a password; a link makes an account
//! for someone new and joins them; ended links are refused; roles are capped.
//! sam owns the server and the workspace; mo is a server member and an
//! admin of the workspace.

use crate::admin_app::roles::{is_guest, role_of};
use crate::admin_app::tests::{server_with, Server};
use crate::team::chat::{Chat, Invite};
use crate::team::{Change, Role};
use axum::Router;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const BASE: &str = "https://magnus.example";

/// A mail server that takes every message and keeps what it was sent.
async fn smtp() -> (u16, Arc<Mutex<Vec<String>>>) {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let got = Arc::new(Mutex::new(vec![]));
    let keep = got.clone();
    tokio::spawn(async move {
        while let Ok((sock, _)) = l.accept().await {
            let keep = keep.clone();
            tokio::spawn(async move {
                let (r, mut w) = sock.into_split();
                let mut lines = BufReader::new(r).lines();
                w.write_all(b"220 fake\r\n").await.unwrap();
                let (mut data, mut body) = (false, String::new());
                while let Ok(Some(line)) = lines.next_line().await {
                    if data {
                        if line == "." {
                            data = false;
                            keep.lock().unwrap().push(std::mem::take(&mut body));
                            w.write_all(b"250 kept\r\n").await.unwrap();
                        } else {
                            body.push_str(&line);
                            body.push('\n');
                        }
                        continue;
                    }
                    let verb = line.split_whitespace().next().unwrap_or_default().to_uppercase();
                    let answer: &[u8] = match verb.as_str() {
                        "EHLO" | "HELO" => b"250 fake\r\n",
                        "DATA" => {
                            data = true;
                            b"354 go\r\n"
                        }
                        "QUIT" => {
                            w.write_all(b"221 bye\r\n").await.unwrap();
                            return;
                        }
                        _ => b"250 ok\r\n",
                    };
                    w.write_all(answer).await.unwrap();
                }
            });
        }
    });
    (port, got)
}

fn mail(port: u16) -> String {
    format!("[mail]\nhost = \"127.0.0.1\"\nport = {port}\ntls = \"none\"\nfrom = \"magnus@example.com\"\nlink_base = \"{BASE}\"\n")
}

/// A workspace sam owns, with mo as its admin, and mo's session.
async fn workspace(s: &Server) -> (String, String) {
    let sam = s.token("sam").await;
    let (st, v) = s.post(&sam, "/v1/workspaces", json!({ "name": "Launch" })).await;
    assert_eq!(st, 200, "{v}");
    let ws = v["id"].as_str().unwrap().to_string();
    assert_eq!(s.post(&sam, &format!("/v1/workspaces/{ws}/members"), json!({ "person": "mo", "role": "admin" })).await.0, 200);
    (ws, s.token("mo").await)
}

fn browser() -> reqwest::Client {
    reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap()
}

/// Post a form; its status, where it sends the browser, whether it set a
/// session, and the page.
async fn form(s: &Server, path: &str, fields: &[(&str, &str)]) -> (u16, String, bool, String) {
    let r = browser().post(format!("{}{path}", s.base)).form(fields).send().await.unwrap();
    let to = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
    let session = r.headers().get("set-cookie").and_then(|v| v.to_str().ok()).is_some_and(|c| c.starts_with(crate::api::SESSION_COOKIE));
    (r.status().as_u16(), to, session, r.text().await.unwrap_or_default())
}

async fn page(s: &Server, path: &str) -> (u16, String) {
    let r = browser().get(format!("{}{path}", s.base)).send().await.unwrap();
    (r.status().as_u16(), r.text().await.unwrap_or_default())
}

fn ws_role(s: &Server, ws: &str, person: &str) -> Option<Role> {
    s.k.read(|st| st.team.role(ws, person))
}

async fn server_role(s: &Server, id: &str) -> &'static str {
    role_of(&s.k.person(id).unwrap(), s.k.accounts.role(id).await)
}

fn token_of(link: &Value) -> String {
    link.as_str().unwrap().split("token=").nth(1).unwrap().to_string()
}

const PW: &str = "a-long-new-password";

/// An unknown address: an account invitation, emailed, and the workspace
/// joined once they choose their password, signed in. A known address
/// joins at once. A guest's invitation makes a server guest.
#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_address_is_emailed_and_joins_on_choosing_a_password() {
    let (port, sent) = smtp().await;
    let s = server_with(&mail(port), |_| Router::new()).await;
    let (ws, mo) = workspace(&s).await;
    let invites = format!("/v1/workspaces/{ws}/invites");
    let (st, v) = s.post(&mo, &invites, json!({ "email": "Newt@Example.com" })).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!((v["invited"].as_str(), v["emailed"].as_bool()), (Some("newt"), Some(true)), "the invitation was not emailed: {v}");
    let link = v["link"].as_str().unwrap().to_string();
    assert!(link.starts_with(&format!("{BASE}/invite?token=")), "{link}");
    // The body is quoted-printable: its soft breaks and `=3D` undone.
    let mails: Vec<String> = sent.lock().unwrap().iter().map(|m| m.replace("=\n", "").replace("=3D", "=")).collect();
    assert_eq!(mails.len(), 1, "one email was not sent");
    assert!(mails[0].contains(&link) && mails[0].contains("Launch"), "the email lacks the link or the workspace: {}", mails[0]);
    assert_eq!(s.k.accounts.status("newt").await, crate::accounts::Status::Invited);
    assert_eq!(ws_role(&s, &ws, "newt"), None, "an invitee joined before choosing a password");
    let token = token_of(&v["link"]);
    let (st, to, session, _) = form(&s, "/invite", &[("token", &token), ("password", PW), ("confirm", PW)]).await;
    assert_eq!((st, to.as_str(), session), (303, format!("/#/team/{ws}").as_str(), true), "choosing a password did not sign in to the workspace");
    assert_eq!(ws_role(&s, &ws, "newt"), Some(Role::Member));
    assert_eq!(server_role(&s, "newt").await, "member");
    let (st, _, _, _) = form(&s, "/invite", &[("token", &token), ("password", PW), ("confirm", PW)]).await;
    assert_eq!(st, 200, "a spent invitation took");
    assert!(!s.k.read(|st| st.team.workspace(&ws).unwrap().chat.invites.values().any(|i| i.email.is_some())), "the bound invite did not end");

    // Known now: joins at once, nothing emailed.
    let other = s.post(&s.token("sam").await, "/v1/workspaces", json!({ "name": "Other" })).await.1["id"].as_str().unwrap().to_string();
    let sam = s.token("sam").await;
    let (_, v) = s.post(&sam, &format!("/v1/workspaces/{other}/invites"), json!({ "email": "newt@example.com" })).await;
    assert_eq!(v["added"], "newt", "{v}");
    assert_eq!(ws_role(&s, &other, "newt"), Some(Role::Member));
    assert_eq!(sent.lock().unwrap().len(), 1, "a known person was emailed an invitation");

    // A guest's invitation makes a guest of the server, never a member.
    let (_, v) = s.post(&mo, &invites, json!({ "email": "gil@example.com", "role": "guest" })).await;
    assert!(is_guest(&s.k.person("gil").unwrap()), "a workspace guest's account is not a guest: {v}");
    s.done().await;
}

/// Mail that fails, or no mail at all: never claimed sent, the link to
/// copy given instead.
#[tokio::test(flavor = "multi_thread")]
async fn mail_that_fails_is_never_claimed_sent() {
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
    let s = server_with(&mail(closed), |_| Router::new()).await;
    let (ws, mo) = workspace(&s).await;
    let (_, v) = s.post(&mo, &format!("/v1/workspaces/{ws}/invites"), json!({ "email": "ida@example.com" })).await;
    assert_eq!(v["emailed"], false, "{v}");
    assert!(v["email_error"].is_string() && v["link"].as_str().is_some_and(|l| l.starts_with(BASE)), "{v}");
    s.done().await;

    let s = server_with("", |_| Router::new()).await;
    let (ws, mo) = workspace(&s).await;
    let (_, v) = s.post(&mo, &format!("/v1/workspaces/{ws}/invites"), json!({ "email": "ida@example.com" })).await;
    assert_eq!((v["emailed"].as_bool(), v["mail"].as_bool()), (Some(false), Some(false)), "{v}");
    assert!(v["path"].as_str().is_some_and(|p| p.starts_with("/invite?token=")), "no link to copy: {v}");
    s.done().await;
}

/// A link: someone new makes their account and joins with its role, a
/// server member however high that role; an ended code is refused and
/// counted; a workspace admin cannot give a role above their own.
#[tokio::test(flavor = "multi_thread")]
async fn a_link_makes_an_account_for_someone_new() {
    let s = server_with(&mail(1), |_| Router::new()).await;
    let (ws, mo) = workspace(&s).await;
    let invites = format!("/v1/workspaces/{ws}/invites");
    assert_eq!(s.post(&mo, &invites, json!({ "role": "owner" })).await.0, 403, "an admin gave the owner role");
    let (st, v) = s.post(&mo, &invites, json!({ "role": "admin" })).await;
    assert_eq!(st, 200, "{v}");
    let code = v["code"].as_str().unwrap().to_string();
    assert_eq!(v["link"].as_str(), Some(format!("{BASE}/join?code={code}").as_str()));
    let (st, body) = page(&s, &format!("/join?code={code}")).await;
    assert!(st == 200 && body.contains("Join Launch"), "{body}");
    let fields = |email: &'static str| vec![("code", code.clone()), ("name", "Ada Lane".into()), ("email", email.into()), ("password", PW.into()), ("confirm", PW.into())];
    let post = |f: Vec<(&'static str, String)>| {
        let s = &s;
        async move { form(s, "/join", &f.iter().map(|(k, v)| (*k, v.as_str())).collect::<Vec<_>>()).await }
    };
    let (st, to, session, _) = post(fields("ada@example.com")).await;
    assert_eq!((st, to, session), (303, format!("/#/team/{ws}"), true), "the newcomer was not signed in to the workspace");
    assert_eq!(ws_role(&s, &ws, "ada"), Some(Role::Admin), "the link's role was not given");
    assert_eq!(server_role(&s, "ada").await, "member", "a workspace admin made a server admin");
    assert_eq!(s.k.accounts.local_person("ada").unwrap().email.as_deref(), Some("ada@example.com"));
    // The link works for the next newcomer too, until it expires.
    assert_eq!(post(fields("bo@example.com")).await.0, 303);
    let (_, _, _, body) = post(fields("ada@example.com")).await;
    assert!(body.contains("already has an account"), "a held address made a second account: {body}");

    // Expired: refused, and counted until the address is held off.
    let past = chrono::Utc::now() - chrono::Duration::minutes(1);
    let old = Invite { code: "old".into(), role: Some(Role::Member), email: None, by: "mo".into(), expires: Some(past) };
    s.k.emit("person:mo", crate::events::Ev::Team(Change::Chat { workspace: ws.clone(), chat: Chat::Invite { invite: old } })).await.unwrap();
    let mut old_fields = fields("cy@example.com");
    old_fields[0].1 = "old".into();
    let (st, _, _, body) = post(old_fields.clone()).await;
    assert!(st == 200 && body.contains("has ended"), "an expired link took: {body}");
    assert!(s.k.person("cy").is_none());
    let mut held = false;
    for _ in 0..30 {
        if post(old_fields.clone()).await.0 == 429 {
            held = true;
            break;
        }
    }
    assert!(held, "guessing codes was never held off");
    s.done().await;
}

/// Admin, People: invite by link with no email. Whoever opens it makes
/// their own account with the link's role, once. With no public address,
/// the answer says there is no link instead of giving a raw code.
#[tokio::test(flavor = "multi_thread")]
async fn an_admin_invites_by_link() {
    let s = server_with(&mail(1), |_| Router::new()).await;
    let ana = s.token("ana").await;
    crate::admin_app::guard::mark(&ana, chrono::Utc::now());
    let (st, v) = s.post(&ana, "/v1/admin/people", json!({ "role": "owner" })).await;
    assert_eq!((st, v["error"].as_str()), (403, Some("owner_only")), "an admin made an owner's link: {v}");
    let (st, v) = s.post(&ana, "/v1/admin/people", json!({ "role": "guest" })).await;
    assert_eq!(st, 201, "{v}");
    let link = v["link"].as_str().unwrap().to_string();
    assert!(link.starts_with(&format!("{BASE}/invite?link=")), "{v}");
    let code = link.split("link=").nth(1).unwrap().to_string();
    let (st, body) = page(&s, &format!("/invite?link={code}")).await;
    assert!(st == 200 && body.contains("name=email"), "the link's page does not ask for an address: {body}");
    let f = [("link", code.as_str()), ("name", "Gus"), ("email", "gus@example.com"), ("password", PW), ("confirm", PW)];
    let (st, to, session, _) = form(&s, "/invite", &f).await;
    assert_eq!((st, to.as_str(), session), (303, "/", true), "the newcomer was not signed in");
    assert!(is_guest(&s.k.person("gus").unwrap()), "the link's guest role was not given");
    let f = [("link", code.as_str()), ("name", "Hal"), ("email", "hal@example.com"), ("password", PW), ("confirm", PW)];
    let (_, _, _, body) = form(&s, "/invite", &f).await;
    assert!(body.contains("spent or expired") && s.k.person("hal").is_none(), "a link worked twice: {body}");
    assert_eq!(s.audited("user_invited").len(), 1, "the link is not in the admin audit");
    s.done().await;

    let s = server_with("", |_| Router::new()).await;
    let sam = s.token("sam").await;
    crate::admin_app::guard::mark(&sam, chrono::Utc::now());
    let (st, v) = s.post(&sam, "/v1/admin/people", json!({ "role": "member", "email": "" })).await;
    assert_eq!((st, v["link"].clone(), v["link_base"].as_bool()), (201, Value::Null, Some(false)), "{v}");
    s.done().await;
}
