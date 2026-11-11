//! A person's own password: changing it in Settings with the current one,
//! and asking for a recovery email when it is forgotten. The recovery page
//! the email links to is `/reset` (api.rs).

use crate::api::{credential, err, internal, on_this_machine, over_tls, who, Transport};
use crate::config::Config;
use crate::events::Ev;
use crate::kernel::Kernel;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;

type K = State<Arc<Kernel>>;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/account/password", get(rules).post(change)).route("/v1/password-reset/request", post(password_reset_request)).merge(crate::mail_settings::router())
}

/// Whether this person's password is kept here, and so changed here: with no
/// identity provider, everyone's; with one, only people the server itself
/// keeps (#429). The owner's backup password under a provider is set on the
/// server's own machine (`magnus password`), never over the network.
async fn kept_here(k: &Kernel, person: &str) -> bool {
    k.identity().is_none() || (k.cfg.person(person).is_none() && k.accounts.is_local_fresh(person).await && k.accounts.has_password(person).await)
}

/// What Settings needs to draw the form: whether the password is changed
/// here, how long a new one must be, and where it is changed otherwise.
async fn rules(State(k): K, h: HeaderMap) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let here = kept_here(&k, &p.id).await;
    Json(json!({ "here": here, "min_length": k.cfg.sign_in.min_password_length, "provider": k.identity().map(|i| i.label()) })).into_response()
}

/// Change the signed-in person's password with their current one. A wrong
/// current password counts as a wrong password at sign-in from the same
/// place does; the new one ends every other session of theirs.
async fn change(State(k): K, Extension(t): Extension<Transport>, ConnectInfo(peer): ConnectInfo<SocketAddr>, h: HeaderMap, Json(b): Json<Value>) -> Response {
    if let Err(r) = over_tls(&k, t, peer) {
        return r;
    }
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    if !kept_here(&k, &p.id).await {
        let at = k.identity().map(|i| i.label()).unwrap_or_else(|| "your sign-in provider".into());
        return err(StatusCode::CONFLICT, format!("Your password is kept by {at}. Change it there."));
    }
    let current = b["current_password"].as_str().unwrap_or_default();
    let new = b["new_password"].as_str().unwrap_or_default();
    let least = k.cfg.sign_in.min_password_length;
    if new.chars().count() < least {
        return err(StatusCode::BAD_REQUEST, format!("The new password needs at least {least} characters."));
    }
    if new == current {
        return err(StatusCode::BAD_REQUEST, "The new password is the same as the current one.");
    }
    let counter = if on_this_machine(peer, &h) { crate::accounts::Accounts::local_counter(&p.id) } else { crate::accounts::Accounts::network_counter(&p.id) };
    let keep = credential(&h).unwrap_or_default();
    match k.accounts.change_password(&p.id, current, new, &counter, keep).await {
        Ok(Ok(ended)) => {
            let actor = format!("person:{}", p.id);
            let mut evs = vec![(actor.as_str(), Ev::PasswordChanged { person: p.id.clone(), how: "settings".into() })];
            evs.extend(ended.iter().map(|session| (actor.as_str(), Ev::SignedOut { person: p.id.clone(), session: session.clone() })));
            if let Err(e) = k.emit_all(evs).await {
                return internal(e);
            }
            Json(json!({ "ok": true, "signed_out": ended.len() })).into_response()
        }
        // Not 401: that would read as the session ending.
        Ok(Err(crate::accounts::SignInError::Locked(until))) => {
            (StatusCode::TOO_MANY_REQUESTS, Json(json!({ "error": format!("Too many wrong passwords. Try again after {}.", until.format("%H:%M UTC")), "retry_at": until })))
                .into_response()
        }
        Ok(Err(_)) => err(StatusCode::FORBIDDEN, "The current password is wrong."),
        Err(e) => internal(e),
    }
}

/// Where account links point: the configured mail base, else the server's
/// public address, else the public hostname when pairing is on. Nothing else
/// is trusted: the request's own host is attacker-controlled, so without
/// one of these no recovery mail goes out at all (#541).
pub(crate) fn link_base(cfg: &Config) -> Option<String> {
    if let Some(base) = cfg.mail.as_ref().and_then(|m| m.link_base.clone()) {
        return Some(base.trim_end_matches('/').to_string());
    }
    if let Some(u) = &cfg.appliance.public_url {
        return Some(u.as_str().to_string());
    }
    cfg.client_certs.as_ref().map(|c| format!("https://{}", c.hostname))
}

/// The recovery email for a token: its subject and text, linking to `base`.
fn recovery_mail(base: &str, token: &str) -> (&'static str, String) {
    let link = format!("{base}/reset?token={token}");
    let text = format!("Someone asked to recover the Magnus password for this address. Choose a new one in the next hour:\n\n{link}\n\nIf that was not you, ignore this email: the password stays as it is. Choosing a new password signs every device out, and any authenticator codes stop working until new ones are enrolled.");
    ("Recover your Magnus password", text)
}

/// Ask for a password recovery email: whoever holds the address gets a
/// one-hour link. The answer never says whether anyone did (#533), and
/// never waits for the mail server: delivery runs behind the response,
/// so active addresses answer no slower than unknown ones (#541).
async fn password_reset_request(State(k): K, Extension(t): Extension<Transport>, ConnectInfo(peer): ConnectInfo<SocketAddr>, Json(b): Json<Value>) -> Response {
    if let Err(r) = over_tls(&k, t, peer) {
        return r;
    }
    let email = b["email"].as_str().unwrap_or_default().trim().to_string();
    let req = match k.accounts.request_reset(&email).await {
        Ok(r) => r,
        Err(e) => return internal(e),
    };
    if let Some(r) = req.filter(|r| r.send) {
        let Some(base) = link_base(&k.cfg) else {
            tracing::warn!("password reset for {}: no trusted link base, nothing sent; set [appliance] public_url", r.person);
            return Json(json!({ "ok": true })).into_response();
        };
        let (subject, text) = recovery_mail(&base, &r.token);
        let (delivery, to) = (k.clone(), email.clone());
        tokio::spawn(async move {
            match crate::mailer::send(&delivery, &to, subject, text).await {
                Ok(true) => {}
                Ok(false) => tracing::warn!("password reset for {}: no [mail] is set, nothing sent", r.person),
                Err(e) => tracing::error!("password reset email to {to} failed: {e:#}"),
            }
        });
    }
    Json(json!({ "ok": true })).into_response()
}

#[cfg(test)]
mod tests {
    use super::{link_base, recovery_mail};

    /// A config with `appliance` lines added to `[appliance]` and `rest`
    /// after it.
    fn load(appliance: &str, rest: &str) -> anyhow::Result<crate::config::Config> {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("magnus.toml");
        let text = format!("[appliance]\nstate_dir = \"/s\"\nlisten = \"127.0.0.1:0\"\n{appliance}\n[database]\nurl = \"postgres://x\"\n[model]\nname = \"m\"\nadapter = \"a\"\nmax_tokens = 1\n{rest}");
        std::fs::write(&p, text).unwrap();
        crate::config::Config::load(&p)
    }

    const MAIL: &str = "[mail]\nhost = \"127.0.0.1\"\nfrom = \"magnus@example.com\"\n";

    /// The recovery link goes to the server's public address when no mail
    /// base is set, a mail base still wins, and with neither (nor pairing)
    /// there is no link at all: the request's host is never used.
    #[test]
    fn the_recovery_link_uses_the_public_address() {
        let public = load("public_url = \"https://teams.pyrallon.ai/\"", MAIL).unwrap();
        assert_eq!(link_base(&public).as_deref(), Some("https://teams.pyrallon.ai"));
        let (_, text) = recovery_mail(&link_base(&public).unwrap(), "abc123");
        assert!(text.contains("\nhttps://teams.pyrallon.ai/reset?token=abc123\n"), "{text}");
        let both = load("public_url = \"https://teams.pyrallon.ai\"", &format!("{MAIL}link_base = \"https://recovery.example/\"\n")).unwrap();
        assert_eq!(link_base(&both).as_deref(), Some("https://recovery.example"));
        assert_eq!(link_base(&load("", MAIL).unwrap()), None);
    }

    /// A public address that is not https, or carries a path, stops the
    /// server at startup instead of mailing a wrong link later.
    #[test]
    fn a_wrong_public_address_is_refused_at_load() {
        for bad in ["http://teams.pyrallon.ai", "teams.pyrallon.ai"] {
            let e = load(&format!("public_url = \"{bad}\""), "").unwrap_err();
            assert!(format!("{e:#}").contains("public_url"), "{bad}: {e:#}");
        }
    }
}
