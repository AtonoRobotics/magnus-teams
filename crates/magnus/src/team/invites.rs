//! Inviting people to a workspace (spec rev 142, Team shared workspace:
//! "Members are invited by email or link"). By email, someone the server
//! knows joins at once; anyone else gets a server account invitation, member
//! or guest, emailed through the same mailer and `/invite` page as the
//! admin's, and joins the workspace when they choose their password. By
//! link, anyone holding it joins: signed in through the app, or with a new
//! account made on the `/join` page (`join_page`).
use super::caller::{at, Member};
use super::chat::{Chat, Invite};
use super::routes::{new_id, send, Reply, K};
use super::{Change, Role};
use crate::api::{err, who};
use crate::api_error::{refuse, ApiResult};
use crate::config::Person;
use crate::events::Ev;
use crate::invitations::{free_id, holder, invite_link, invite_path, Delivery};
use crate::kernel::Kernel;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

/// How long a link stays good.
pub(super) const LINK_LIFETIME_DAYS: i64 = 7;
const MAX_NAME_CHARS: usize = 200;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/workspaces/{ws}/invites", post(invite)).route("/v1/invites/{code}/join", post(join)).merge(super::join_page::routes())
}

/// What accepting an invitation did: who it activated and the workspaces
/// they joined with it.
#[derive(Debug, Clone, PartialEq)]
pub struct Accepted {
    pub person: String,
    pub workspaces: Vec<String>,
}

impl Kernel {
    /// Accept an account invitation, as `Accounts::accept` does, and join
    /// every workspace whose live invite is bound to the person's address.
    pub async fn accept_invitation(&self, token: &str, password: &str) -> anyhow::Result<Option<Accepted>> {
        let Some(person) = self.accounts.accept(token, password).await? else { return Ok(None) };
        let workspaces = match self.join_bound(&person).await {
            Ok(w) => w,
            Err(e) => {
                tracing::error!("joining {person}'s workspaces after their invitation: {e:#}");
                vec![]
            }
        };
        Ok(Some(Accepted { person, workspaces }))
    }

    /// Join the workspaces whose live invite is bound to this person's
    /// address, each with the role it carries; each invite then ends.
    async fn join_bound(&self, person: &str) -> anyhow::Result<Vec<String>> {
        let Some(email) = self.accounts.local_person(person).and_then(|l| l.email) else { return Ok(vec![]) };
        let now = chrono::Utc::now();
        let found: Vec<(String, Invite, bool)> = self.read(|s| {
            s.team
                .all()
                .flat_map(|w| {
                    w.chat
                        .invites
                        .values()
                        .filter(|i| i.expires.is_none_or(|e| e > now) && i.email.as_deref().is_some_and(|e| e.eq_ignore_ascii_case(&email)))
                        .map(|i| (w.id.clone(), i.clone(), w.members.contains_key(person)))
                        .collect::<Vec<_>>()
                })
                .collect()
        });
        let bound = found.into_iter().filter_map(|(ws, inv, already)| admits(self, &ws, &inv).map(|role| (ws, inv, already, role)));
        let actor = format!("person:{person}");
        let mut joined = vec![];
        for (ws, inv, already, role) in bound.collect::<Vec<_>>() {
            if !already {
                self.emit(&actor, Ev::Team(Change::Member { workspace: ws.clone(), person: person.into(), role })).await?;
            }
            self.emit(&actor, Ev::Team(Change::Chat { workspace: ws.clone(), chat: Chat::InviteEnded { code: inv.code } })).await?;
            joined.push(ws);
        }
        Ok(joined)
    }
}

/// Make an invited account for someone new to the server: a member, or a
/// guest when `guest`, never more, whoever invites them. A guest whose role
/// does not take is not left a member: the account goes again.
pub(super) async fn new_account(k: &Arc<Kernel>, actor: &str, email: &str, name: &str, guest: bool) -> Result<(String, String), (StatusCode, String)> {
    let Some(id) = free_id(k, email) else { return Err((StatusCode::BAD_REQUEST, "No account name derives from that address.".into())) };
    let token = k.invite_account(&id, name, email, crate::accounts::Role::Member).await.map_err(|(c, _, m)| (c, m))?;
    if guest {
        if let Err((c, _, m)) = k.set_server_role(actor, &id, "guest").await {
            if let Err(e) = k.accounts.remove_person(&id).await {
                tracing::error!("taking back {id}'s account after its guest role failed: {e:#}");
            }
            return Err((c, m));
        }
    }
    Ok((id, token))
}

/// A live invite that still admits someone, its workspace and its name, and
/// the role it admits them with (`admits`).
pub(super) fn live(k: &Kernel, code: &str) -> Option<(String, String, Invite, Role)> {
    let now = chrono::Utc::now();
    let (ws, name, inv) = k.read(|s| s.team.all().find_map(|w| w.invite(code, now).map(|i| (w.id.clone(), w.name.clone(), i.clone()))))?;
    let role = admits(k, &ws, &inv)?;
    Some((ws, name, inv, role))
}

/// The role an invite lets someone into `ws` with, if it still lets anyone
/// in: only while whoever made it is still on the server and in the
/// workspace, never above the role they hold there now, and only as a guest
/// when the server holds them to a guest's limits. Every way in asks this:
/// the signed-in join, the link's page and an account invitation accepted.
pub(super) fn admits(k: &Kernel, ws: &str, inv: &Invite) -> Option<Role> {
    let issuer = k.person(&inv.by)?;
    let theirs = k.read(|s| s.team.role(ws, &inv.by))?;
    if crate::people::rights(&issuer).held_to_guest_limits() {
        return Some(Role::Guest);
    }
    Some(inv.role.unwrap_or(Role::Member).min(theirs))
}

#[derive(Deserialize)]
struct NewInvite {
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    role: Option<Role>,
    /// The name of someone new to the server; their address's name if none.
    #[serde(default)]
    name: Option<String>,
}

/// By email: someone the server already knows joins at once; anyone else
/// gets an account invitation by email. Without an email, a link.
async fn invite(State(k): K, Member { person: p, role: mine, .. }: Member<at::Member>, Path(ws): Path<String>, Json(b): Json<NewInvite>) -> ApiResult {
    let role = b.role.unwrap_or(Role::Member);
    if role > mine {
        return refuse(StatusCode::FORBIDDEN, "you cannot give a role above your own");
    }
    let Some(email) = b.email.map(|e| e.trim().to_lowercase()).filter(|e| !e.is_empty()) else { return Ok(by_link(&k, &p, ws, role).await) };
    if !crate::invitations::valid_email(&email) {
        return refuse(StatusCode::BAD_REQUEST, "that is not an email address");
    }
    if let Some(person) = holder(&k, &email) {
        let already = k.read(|s| s.team.role(&ws, &person)).is_some();
        if !already {
            send(&k, &p, Change::Member { workspace: ws, person: person.clone(), role }).await?;
        }
        return Ok(Json(json!({ "added": person, "already": already })).into_response());
    }
    let name = b.name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty() && n.chars().count() <= MAX_NAME_CHARS);
    Ok(newcomer(&k, &p, ws, role, email, name).await)
}

/// Someone new to the server: their account invitation, bound to this
/// workspace and role, and the email with its link.
async fn newcomer(k: &Arc<Kernel>, p: &Person, ws: String, role: Role, email: String, name: Option<String>) -> Reply {
    let name = name.unwrap_or_else(|| email.split('@').next().unwrap_or_default().to_string());
    // A server guest brings in only guests: their invitation never makes an
    // account with more of the server than their own.
    let guest = role == Role::Guest || crate::people::rights(p).held_to_guest_limits();
    let (id, token) = match new_account(k, &format!("person:{}", p.id), &email, &name, guest).await {
        Ok(made) => made,
        Err((code, message)) => return err(code, message),
    };
    let expires = chrono::Utc::now() + chrono::Duration::days(crate::accounts::INVITE_DAYS);
    let invite = Invite { code: new_id(), role: Some(if guest { Role::Guest } else { role }), email: Some(email.clone()), by: p.id.clone(), expires: Some(expires) };
    let workspace = k.read(|s| s.team.workspace(&ws).map(|w| w.name.clone())).unwrap_or_default();
    ok_or_return!(send(k, p, Change::Chat { workspace: ws, chat: Chat::Invite { invite } }).await);
    let link = invite_link(&k.cfg, &token);
    let inviter = if p.name.is_empty() { p.id.clone() } else { p.name.clone() };
    let sent = match (&link, k.cfg.mail.is_some()) {
        (Some(l), _) => crate::invitations::email(k, &inviter, &email, &id, &format!("the {workspace} workspace in Magnus"), l).await,
        (None, true) => Delivery { emailed: false, error: Some("the server has no address to put in the link: set [appliance] public_url".into()) },
        (None, false) => Delivery::default(),
    };
    Json(json!({
        "invited": id, "emailed": sent.emailed, "mail": k.cfg.mail.is_some(), "email_error": sent.error,
        "link": link, "path": invite_path(&token), "expires": expires,
    }))
    .into_response()
}

/// A link anyone holding it joins with, until it expires, with this role.
async fn by_link(k: &Kernel, p: &Person, ws: String, role: Role) -> Reply {
    // A server guest's link brings in only guests, as their email does.
    let role = if crate::people::rights(p).held_to_guest_limits() { Role::Guest } else { role };
    let expires = chrono::Utc::now() + chrono::Duration::days(LINK_LIFETIME_DAYS);
    let invite = Invite { code: new_id(), role: Some(role), email: None, by: p.id.clone(), expires: Some(expires) };
    let code = invite.code.clone();
    ok_or_return!(send(k, p, Change::Chat { workspace: ws, chat: Chat::Invite { invite } }).await);
    let path = super::join_page::join_path(&code);
    let link = crate::passwords::link_base(&k.cfg).map(|base| format!("{base}{path}"));
    Json(json!({ "code": code, "link": link, "path": path, "expires": expires })).into_response()
}

/// Whoever is signed in and holds a live code joins. A code for one
/// address is used once, and only by the holder of that address; a link
/// works until it expires.
async fn join(State(k): K, h: HeaderMap, Path(code): Path<String>) -> Reply {
    let p = ok_or_return!(who(&k, &h).await);
    let Some((ws, _, inv, role)) = live(&k, &code) else { return err(StatusCode::NOT_FOUND, "that invite has ended") };
    let mine = k.accounts.local_person(&p.id).and_then(|l| l.email);
    if inv.email.as_deref().is_some_and(|e| mine.as_deref().is_none_or(|m| !m.eq_ignore_ascii_case(e))) {
        return err(StatusCode::NOT_FOUND, "that invite has ended");
    }
    if k.read(|s| s.team.role(&ws, &p.id)).is_none() {
        ok_or_return!(send(&k, &p, Change::Member { workspace: ws.clone(), person: p.id.clone(), role }).await);
        // A newcomer's own computer is there from the start too.
        k.computer_soon(crate::computer::Owner::Person(p.id.clone()));
    }
    if inv.email.is_some() {
        ok_or_return!(send(&k, &p, Change::Chat { workspace: ws.clone(), chat: Chat::InviteEnded { code } }).await);
    }
    Json(json!({ "workspace": ws })).into_response()
}
