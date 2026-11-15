//! The person's apps over MCP, added and signed in to from the app (the
//! record's product scope: email, calendar, files, chat, business systems).
//!
//! Apps in the config file stay as written there; apps the owner adds live in
//! the log. Sign-in follows MCP's authorization spec (2025-06-18): the app's
//! protected-resource metadata names its authorization server; Magnus
//! registers itself there (RFC 7591) unless the owner gave a client id, and
//! signs in with the authorization code and PKCE, the code coming back to a
//! loopback address on the person's computer (RFC 8252), where the desktop
//! app catches it. Tokens are kept in the vault and refreshed when they
//! expire; the kill switch revokes them (RFC 7009).

use crate::config::Connector;
use crate::kernel::Kernel;
use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::Duration;

mod admin;
mod discovery;
pub(crate) mod relay;
mod requests;
mod routes;
mod sign_in;
mod sign_in_target;
mod slack;
mod standing;

pub use admin::{org_credential, OrgCredential, Refusal};
pub use routes::router;
pub use standing::revoke;

pub const OAUTH: &str = "oauth";

/// The redirect Slack registers for a distributed app. The token exchange
/// sends this same address. The browser is sent on to `SLACK_HANDOFF`,
/// which the desktop app is listening on.
pub const SLACK_OAUTH_CALLBACK: &str = "https://magnus.pyrallon.ai/oauth/slack/callback";
pub const SLACK_HANDOFF: &str = "http://127.0.0.1:17321/callback";

/// The vault name of an app's credential: the organisation's key for an
/// app that takes one, or, before connections were each person's own, the
/// one sign-in everyone's runs used (no longer used; revoked with the app).
pub fn credential_name(app: &str) -> String {
    format!("connector_{app}")
}

/// The vault name of one person's own sign-in to an app (connectors spec:
/// the organisation provides the connector, each person signs in with
/// their own account, and no one else acts through it). App names never
/// hold `__`, so the person is what follows the first one.
pub fn connection_name(app: &str, person: &str) -> String {
    format!("connector_{app}__{person}")
}

/// Whether a vault name is one of an app's sign-ins: a person's own, or
/// the old shared one.
pub fn is_sign_in_of(name: &str, app: &str) -> bool {
    name == credential_name(app) || name.starts_with(&format!("{}__", credential_name(app)))
}

/// Where one person stands with one of the organisation's apps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Standing {
    /// Signed in and working.
    Connected,
    /// Signed in once, and the sign-in no longer works (refused at refresh,
    /// or revoked at the app): one button signs in again.
    NeedsAttention,
    /// Not signed in.
    Available,
}

impl Standing {
    pub fn as_str(self) -> &'static str {
        match self {
            Standing::Connected => "connected",
            Standing::NeedsAttention => "needs_attention",
            Standing::Available => "available",
        }
    }
}

/// An app's name as people read it: "google-calendar" is "Google calendar".
pub fn title(name: &str) -> String {
    let words = name.replace(['-', '_'], " ");
    let mut c = words.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// What an app lets Magnus do, in plain words, from the classes of its
/// tools: one sentence for its card, and one line per kind of action.
pub fn allows(c: &Connector) -> (String, Vec<String>) {
    const KINDS: [(&str, &str, &str); 4] = [
        ("read", "read", "Reads"),
        ("reversible_write", "make changes that can be undone", "Changes that can be undone"),
        ("irreversible", "make changes that cannot be undone, when allowed", "Changes that cannot be undone"),
        ("financial", "move money, when allowed", "Moves money"),
    ];
    let mut can = vec![];
    let mut lines = vec![];
    for (risk, verb, head) in KINDS {
        let tools: Vec<String> = c.tools.iter().filter(|(_, t)| t.risk == risk).map(|(n, _)| n.replace(['_', '-'], " ")).collect();
        if !tools.is_empty() {
            can.push(verb);
            lines.push(format!("{head}: {}", tools.join(", ")));
        }
    }
    let name = title(&c.name);
    let sentence = match can.as_slice() {
        [] => format!("Magnus can work in {name} once your organisation sets it up."),
        [one] => format!("Magnus can {one} in {name}."),
        [rest @ .., last] => format!("Magnus can {} and {last} in {name}.", rest.join(", ")),
    };
    (sentence, lines)
}

/// A sign-in left unfinished this long is dropped.
pub(crate) const PENDING_FOR: Duration = Duration::from_secs(15 * 60);
pub(crate) const CONNECT_WAIT: Duration = Duration::from_secs(10); // a call to a provider: the connection, then the whole call
pub(crate) const CALL_WAIT: Duration = Duration::from_secs(30);
pub(crate) const RETRY_AFTER_MOST: Duration = Duration::from_secs(10); // the most of a registration's `Retry-After` that is waited
                                                                       // `until_client_known`: how often the sign-in is asked, how long in all, and each ask's limit.
pub(crate) const KNOWN_POLL: Duration = Duration::from_millis(250);
pub(crate) const KNOWN_WAIT: Duration = Duration::from_secs(10);
pub(crate) const PROBE_WAIT: Duration = Duration::from_secs(10);
/// A refused registration's first wait in seconds, doubled for each try after up to the third.
pub(crate) const BACKOFF_FIRST_SECS: u64 = 1;

/// One refresh at a time: apps that rotate refresh tokens accept each once.
pub(crate) fn refreshing() -> &'static tokio::sync::Mutex<()> {
    static R: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    R.get_or_init(Default::default)
}

pub(crate) fn http() -> reqwest::Client {
    reqwest::Client::builder().connect_timeout(CONNECT_WAIT).timeout(CALL_WAIT).build().unwrap_or_default()
}

impl Kernel {
    /// The organisation's apps that are on: the config file's, those the
    /// owner added, and the whole catalog, already set up (connectors spec
    /// rev 46), each with who may use it as an admin set it. An app an
    /// admin turned off is offered to no one and reached by no run.
    pub fn connectors(&self) -> Vec<Connector> {
        self.all_connectors().into_iter().filter(|(_, on)| *on).map(|(c, _)| c).collect()
    }

    /// Every app, on or off (admin console spec, Security › Connectors and credentials).
    pub fn all_connectors(&self) -> Vec<(Connector, bool)> {
        let mut all = self.cfg.connectors.clone();
        let (added, settings) = self.read(|s| (s.apps.connectors().values().cloned().collect::<Vec<Connector>>(), s.apps.connector_settings().clone()));
        all.extend(added.into_iter().filter(|c| self.cfg.connectors.iter().all(|f| f.name != c.name)));
        for m in crate::catalog::manifests() {
            // A withdrawn one is offered to no one and reached by no run (`catalog_withdrawal`).
            if all.iter().all(|c| c.name != m.id) && !self.read(|st| st.catalog.covers(m)) {
                all.push(crate::catalog::connector(m));
            }
        }
        all.into_iter()
            .map(|mut c| match settings.get(&c.name) {
                Some(set) => {
                    if let Some(g) = &set.access_groups {
                        c.access_groups = g.clone();
                    }
                    if let Some(g) = &set.blocked_groups {
                        c.blocked_groups = g.clone();
                    }
                    (c, set.enabled)
                }
                None => (c, true),
            })
            .collect()
    }

    /// Whether a catalog connector's provider client is complete on this
    /// appliance: its secret, when it has one, provisioned by Alpha Vector,
    /// and its client ID, when the catalog names none. Only a deployment
    /// that has not yet had them is incomplete.
    pub fn provider_ready(&self, c: &Connector) -> bool {
        match c.registration.as_deref() {
            None => true,
            Some(p) => crate::catalog::client(p).is_some_and(|cl| {
                (!cl.needs_secret() || self.cfg.relay.as_ref().is_some_and(|r| r.relays(p)) || self.read(|s| s.apps.registrations().contains(p)))
                    && (cl.client_id.is_some() || crate::registrations::client(&self.vault, p).is_ok())
            }),
        }
    }

    pub fn connector(&self, name: &str) -> Option<Connector> {
        self.connectors().into_iter().find(|c| c.name == name)
    }
}

/// The owner's starting classification for a tool, from the app's own hints
/// (MCP tool annotations). Hints are the app's claims, so a tool that says
/// nothing is assumed to do the most.
pub fn suggested_risk(annotations: &Value) -> &'static str {
    match (annotations["readOnlyHint"].as_bool(), annotations["destructiveHint"].as_bool()) {
        (Some(true), _) => "read",
        (_, Some(false)) => "reversible_write",
        _ => "irreversible",
    }
}

/// What the Library shows of an app beyond its card's sentence (connectors
/// spec, Library): from its catalog manifest; an app the organisation added
/// by its address has no publisher, and no review mark.
pub(crate) fn listing(c: &Connector) -> Value {
    match crate::catalog::manifest(&c.name).filter(|_| c.catalog) {
        Some(m) => json!({
            "publisher": m.publisher, "reviewed": true, "category": m.category, "abilities": m.abilities,
            "asks": m.asks, "can": m.can, "cannot": m.cannot, "letters": m.letters, "tint": m.tint,
        }),
        None => {
            let t = c.title.clone().unwrap_or_else(|| title(&c.name));
            json!({ "publisher": null, "reviewed": false, "category": "work", "abilities": [], "asks": [], "can": null, "cannot": null, "letters": t.chars().take(2).collect::<String>(), "tint": "#3A3F48" })
        }
    }
}

#[cfg(test)]
mod naming;
