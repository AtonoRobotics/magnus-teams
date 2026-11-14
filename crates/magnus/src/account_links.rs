//! Inviting someone to the server by link (spec rev 142, Administration,
//! People: "Invite by email or link"). An admin makes a link that carries a
//! role; whoever opens it on the `/invite` page gives their name, email
//! address and password, and their account is made with that role. A link
//! works once and for `crate::accounts::INVITE_DAYS` days. The log keeps
//! only its code's hash, as the accounts store keeps a token's.
//!
//! Attempts are counted like wrong passwords at sign-in
//! (`Accounts::guessed_wrong`, per address), so codes cannot be guessed at speed.

use crate::admin_app::invite_page::{escape, shell, signed_in, INVITE_PATH};
use crate::api::Transport;
use crate::events::Ev;
use crate::kernel::Kernel;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::sync::Arc;

/// One change to the server's invite links.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "what", rename_all = "snake_case")]
pub enum Change {
    /// A link made by `by`, giving `role` (owner, admin, member or guest).
    Made { hash: String, role: String, by: String, expires: DateTime<Utc> },
    /// A link spent: its account was made.
    Used { hash: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub role: String,
    pub by: String,
    pub expires: DateTime<Utc>,
}

/// The links not yet spent, by their code's hash.
#[derive(Default, Clone, Debug)]
pub struct Ledger {
    links: BTreeMap<String, Link>,
}

/// Nothing: an invite link is the server's, and who made it is a record of
/// what they did, as an approval they gave others stays (#191).
impl crate::forget::Forget for Ledger {
    fn forget(&mut self, _: &crate::forget::Erased) {}
}

impl Ledger {
    pub fn apply(&mut self, c: &Change) {
        match c {
            Change::Made { hash, role, by, expires } => {
                self.links.insert(hash.clone(), Link { role: role.clone(), by: by.clone(), expires: *expires });
            }
            Change::Used { hash } => {
                self.links.remove(hash);
            }
        }
    }

    /// The link a code opens, while it is unspent and unexpired.
    pub fn live(&self, code: &str, now: DateTime<Utc>) -> Option<&Link> {
        self.links.get(&hash(code)).filter(|l| l.expires > now)
    }
}

fn hash(code: &str) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(code.trim().as_bytes()))
}

/// The link's path below the server's address.
pub fn link_path(code: &str) -> String {
    format!("{INVITE_PATH}?link={code}")
}

/// Accepting is one at a time, so a link is spent exactly once.
static SPENDING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Make a link for `role`, as the admin answers it: the link when the server
/// has a public address, and plainly that it has none when it does not.
pub async fn made(k: &Kernel, actor: &str, role: &str) -> Response {
    let mut raw = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut raw);
    let code = hex::encode(raw);
    let expires = Utc::now() + chrono::Duration::days(crate::accounts::INVITE_DAYS);
    let by = actor.split_once(':').map_or(actor, |(_, id)| id).to_string();
    if let Err(e) = k.emit(actor, Ev::AccountLink(Change::Made { hash: hash(&code), role: role.into(), by, expires })).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("{e:#}") }))).into_response();
    }
    let link = crate::passwords::link_base(&k.cfg).map(|base| format!("{base}{}", link_path(&code)));
    let body = json!({ "by_link": true, "role": role, "link": link, "link_base": link.is_some(), "path": link_path(&code), "emailed": false, "expires": expires });
    (StatusCode::CREATED, Json(body)).into_response()
}

fn form(code: &str, error: Option<&str>) -> Response {
    let error = error.map(|e| format!("<p class=error>{}</p>", escape(e))).unwrap_or_default();
    shell(
        "Join Magnus",
        &format!(
            "<h1>Join Magnus</h1><p>Make your account. You sign in with your email address and the password you choose.</p>{error}<form method=post action={INVITE_PATH}><input type=hidden name=link value=\"{}\">{}<button>Join</button></form>",
            escape(code),
            crate::invitations::signup_fields()
        ),
    )
}

fn ended() -> Response {
    shell("Invitation", "<h1>This invitation is spent or expired.</h1><p>Ask the person who invited you to send it again.</p>")
}

fn too_many(until: DateTime<Utc>) -> Response {
    let mut r = shell("Invitation", &format!("<h1>Too many tries.</h1><p>Try again after {}.</p>", until.format("%H:%M UTC")));
    *r.status_mut() = StatusCode::TOO_MANY_REQUESTS;
    r
}

fn live(k: &Kernel, code: &str) -> Option<Link> {
    k.read(|s| s.account_links.live(code, Utc::now()).cloned())
}

/// The form behind a link, or why there is none.
pub fn page(k: &Kernel, peer: SocketAddr, code: &str) -> Response {
    if let Some(until) = k.accounts.guesses_locked(peer.ip()) {
        return too_many(until);
    }
    if live(k, code).is_none() {
        k.accounts.guessed_wrong(peer.ip());
        return ended();
    }
    form(code, None)
}

/// Make the account a link gives, sign them in, and spend the link.
pub async fn create(k: &Arc<Kernel>, t: Transport, peer: SocketAddr, f: &HashMap<String, String>) -> Response {
    if let Some(until) = k.accounts.guesses_locked(peer.ip()) {
        return too_many(until);
    }
    let code = f.get("link").map(|s| s.trim().to_string()).unwrap_or_default();
    let who = match crate::invitations::signup(k, f) {
        Ok(who) => who,
        Err(why) if live(k, &code).is_some() => return form(&code, Some(&why)),
        Err(_) => {
            k.accounts.guessed_wrong(peer.ip());
            return ended();
        }
    };
    let spending = SPENDING.lock().await;
    let Some(link) = live(k, &code) else {
        k.accounts.guessed_wrong(peer.ip());
        return ended();
    };
    let wrong = || form(&code, Some("Something went wrong. Try again in a minute."));
    let Some(id) = crate::invitations::free_id(k, &who.email) else { return form(&code, Some("No account name derives from that address.")) };
    let directory = if matches!(link.role.as_str(), "owner" | "admin") { crate::accounts::Role::Admin } else { crate::accounts::Role::Member };
    let token = match k.invite_account(&id, &who.name, &who.email, directory).await {
        Ok(t) => t,
        Err((_, _, m)) => {
            tracing::error!("an account from an invite link: {m}");
            return wrong();
        }
    };
    if let Err((_, _, m)) = k.set_server_role(&format!("person:{}", link.by), &id, &link.role).await {
        tracing::error!("an account from an invite link, its role: {m}");
        if let Err(e) = k.accounts.remove_person(&id).await {
            tracing::error!("taking back {id}'s account: {e:#}");
        }
        return wrong();
    }
    if let Err(e) = k.emit(&format!("person:{id}"), Ev::AccountLink(Change::Used { hash: hash(&code) })).await {
        tracing::error!("spending an invite link: {e:#}");
        return wrong();
    }
    drop(spending);
    match k.accounts.accept(&token, &who.password).await {
        Ok(Some(_)) => signed_in(k, t, &id, None).await,
        _ => wrong(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_works_once_and_until_it_expires() {
        let now = Utc::now();
        let mut l = Ledger::default();
        l.apply(&Change::Made { hash: hash("abc"), role: "member".into(), by: "sam".into(), expires: now + chrono::Duration::days(1) });
        assert!(l.live("abc", now).is_some());
        assert!(l.live("abd", now).is_none(), "another code opened it");
        assert!(l.live("abc", now + chrono::Duration::days(2)).is_none(), "an expired link opened");
        l.apply(&Change::Used { hash: hash("abc") });
        assert!(l.live("abc", now).is_none(), "a spent link opened again");
    }
}
