//! Account invitations, one path for every way someone is brought in (spec
//! rev 142, Administration, People: "Invite by email or link"; Team shared
//! workspace: "Members are invited by email or link"). The admin's People
//! screen and a workspace's invite both make the account here, link to the
//! same `/invite` page, and email it through the same mailer.

use crate::accounts::{Role, INVITE_DAYS};
use crate::admin_app::roles::Refusal;
use crate::config::Config;
use crate::kernel::Kernel;
use axum::http::StatusCode;

/// The longest person id the accounts store takes.
const MAX_PERSON_ID_CHARS: usize = 64;
/// The longest email address there is (RFC 5321).
const MAX_EMAIL_CHARS: usize = 254;
/// How many numbered ids (`ann-2` ... `ann-9`) a workspace invite tries
/// when the address's own id is taken.
const MAX_ID_SUFFIX: u32 = 9;

/// Whether this reads as one email address.
pub fn valid_email(e: &str) -> bool {
    let Some((user, host)) = e.split_once('@') else { return false };
    !user.is_empty()
        && host.contains('.')
        && !host.starts_with('.')
        && !host.ends_with('.')
        && e.len() <= MAX_EMAIL_CHARS
        && !e.chars().any(|c| c.is_whitespace() || c == '<' || c == '>' || c == ',')
}

/// The person id an address's local part gives, or none.
pub fn derive_id(email: &str) -> Option<String> {
    let id: String = email.split('@').next().unwrap_or_default().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_' || *c == '-').collect();
    let id = id.to_lowercase();
    (!id.is_empty() && id.len() <= MAX_PERSON_ID_CHARS).then_some(id)
}

/// An id nobody holds for an address: its own, else the first free
/// numbered one. The admin's invite refuses a held id instead; a workspace
/// admin does not pick ids, so theirs takes the next free one.
/// An erased person's id is never given again: whatever of theirs outlived
/// the erasure under that id (a rule, a stored card, an app they built) must
/// not reach someone new.
pub fn free_id(k: &Kernel, email: &str) -> Option<String> {
    let base = derive_id(email)?;
    let held = |id: &str| k.person(id).is_some() || k.read(|s| s.erasures.contains_key(id));
    std::iter::once(base.clone()).chain((2..=MAX_ID_SUFFIX).map(|n| format!("{base}-{n}"))).find(|id| id.len() <= MAX_PERSON_ID_CHARS && !held(id))
}

/// The local person who holds this address, if any.
pub fn holder(k: &Kernel, email: &str) -> Option<String> {
    k.accounts.local_people().into_iter().find(|(_, lp)| lp.email.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(email))).map(|(id, _)| id)
}

/// Where an invitation's token is used: the `/invite` page, below the
/// trusted link base, or none when the server has no address to give.
pub fn invite_link(cfg: &Config, token: &str) -> Option<String> {
    crate::passwords::link_base(cfg).map(|base| format!("{base}{}", invite_path(token)))
}

/// `invite_link` below the server's address, for a client that knows it.
pub fn invite_path(token: &str) -> String {
    format!("{}?token={token}", crate::admin_app::invite_page::INVITE_PATH)
}

/// Whether an invitation went out by email, and why not when it failed.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Delivery {
    pub emailed: bool,
    pub error: Option<String>,
}

/// Email an invitation's link: `inviter` invited them to `into`, as the
/// account `person`. With no `[mail]`, nothing is sent and nothing is claimed.
pub async fn email(k: &Kernel, inviter: &str, to: &str, person: &str, into: &str, link: &str) -> Delivery {
    let text = format!("{inviter} invited you to {into}. Choose your password within {INVITE_DAYS} days to start:\n\n{link}\n\nAfterwards, sign in with this email address or your username, {person}.\n\nIf you did not expect this, ignore this email.");
    match crate::mailer::send(k, to, &format!("You are invited to {into}"), text).await {
        Ok(sent) => Delivery { emailed: sent, error: None },
        Err(e) => {
            tracing::warn!("invitation email to {to} failed: {e:#}");
            Delivery { emailed: false, error: Some(format!("{e:#}")) }
        }
    }
}

impl Kernel {
    /// Make an invited account and its single-use token, as the admin's
    /// People screen does: the person, their invited status, their
    /// directory role and the token in one transaction, and their computer
    /// made soon after. A held id refuses `taken`.
    pub async fn invite_account(self: &std::sync::Arc<Self>, id: &str, name: &str, email: &str, role: Role) -> Result<String, Refusal> {
        match self.accounts.invite_person(id, name, Some(email), role).await {
            Ok(token) => {
                self.computer_soon(crate::computer::Owner::Person(id.to_string()));
                Ok(token)
            }
            Err(e) => {
                let message = format!("{e:#}");
                if message.contains("already a person") {
                    return Err((StatusCode::CONFLICT, "taken", message));
                }
                Err((StatusCode::INTERNAL_SERVER_ERROR, "failed", message))
            }
        }
    }
}

/// The longest name a person gives themselves on a sign-up form.
pub const MAX_NAME_CHARS: usize = 200;

/// The counter a sign-up form's attempts from this address are counted on,
/// as wrong passwords are at sign-in: an unknown or ended code counts.
pub fn attempts(ip: std::net::IpAddr) -> String {
    format!("signup:{ip}")
}

/// What someone with a link gives to make their account.
#[derive(Debug, Clone, PartialEq)]
pub struct Signup {
    pub name: String,
    pub email: String,
    pub password: String,
}

/// A sign-up form's fields, or what to tell them to fix.
pub fn signup(k: &Kernel, f: &std::collections::HashMap<String, String>) -> Result<Signup, String> {
    let field = |n: &str| f.get(n).map(|s| s.trim().to_string()).unwrap_or_default();
    let (name, email) = (field("name"), field("email").to_lowercase());
    let password = f.get("password").cloned().unwrap_or_default();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return Err("Give the name people here will know you by.".into());
    }
    if !valid_email(&email) {
        return Err("That is not an email address.".into());
    }
    if f.get("confirm") != Some(&password) {
        return Err("The two passwords do not match.".into());
    }
    let min = k.cfg.sign_in.min_password_length;
    if password.chars().count() < min {
        return Err(format!("A password needs at least {min} characters."));
    }
    if holder(k, &email).is_some() {
        return Err("That address already has an account here. Sign in to Magnus instead.".into());
    }
    Ok(Signup { name, email, password })
}

/// The sign-up fields of a form, after its hidden ones.
pub fn signup_fields() -> String {
    format!(
        "<label>Your name<input name=name autocomplete=name maxlength={MAX_NAME_CHARS} required></label><label>Email address<input type=email name=email autocomplete=email required></label><label>Password<input type=password name=password autocomplete=new-password required></label><label>Repeat it<input type=password name=confirm autocomplete=new-password required></label>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_comes_from_the_address() {
        assert_eq!(derive_id("Ann.Lee+x@example.com").as_deref(), Some("ann.leex"));
        assert_eq!(derive_id("@example.com"), None);
        assert!(valid_email("ann@example.com"));
        assert!(!valid_email("ann@example"));
        assert!(!valid_email("a b@example.com"));
    }

    /// Someone new with an erased person's address does not get their id,
    /// and with it whatever of theirs outlived the erasure.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_erased_persons_id_goes_to_nobody_new() {
        let s = crate::admin_app::tests::server_with("", |_| axum::Router::new()).await;
        assert_eq!(free_id(&s.k, "ada@example.com").as_deref(), Some("ada"));
        s.k.emit("person:sam", crate::events::Ev::Removed { item: "person:ada".into(), seqs: vec![] }).await.unwrap();
        assert_eq!(free_id(&s.k, "ada@elsewhere.com").as_deref(), Some("ada-2"), "an erased person's id was given again");
        s.done().await;
    }
}
