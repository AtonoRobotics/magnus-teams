//! The connector catalog: the manifests of the connectors that ship in
//! Magnus (`catalog/*.toml`), compiled into the appliance so the release's
//! signature covers them (connectors spec, "Catalog and connector
//! manifests"). A manifest's tool classes count as the organisation owner's
//! classification (owner's ruling 5). Every catalog connector is in every
//! Library, already set up: no one, person or admin, is asked for a client
//! ID, secret or address (connectors spec rev 46, "What the user sees").
//! A connector whose provider needs Alpha Vector's own client signs in
//! through it (`catalog/registrations.toml`); its secret, when it has one,
//! is Alpha Vector's, provisioned to the appliance until the relay holds it.

use crate::config::{Connector, ToolClass};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

const SOURCES: &[&str] = &[
    include_str!("../../../catalog/google-gmail.toml"),
    include_str!("../../../catalog/google-calendar.toml"),
    include_str!("../../../catalog/google-drive.toml"),
    include_str!("../../../catalog/google-chat.toml"),
    include_str!("../../../catalog/sentry.toml"),
    include_str!("../../../catalog/huggingface.toml"),
    include_str!("../../../catalog/gitlab.toml"),
    include_str!("../../../catalog/supabase.toml"),
    include_str!("../../../catalog/cloudflare.toml"),
    include_str!("../../../catalog/slack.toml"),
];

#[derive(Deserialize, Clone, Debug)]
pub struct Manifest {
    pub id: String,
    pub version: String,
    pub name: String,
    pub summary: String,
    /// The Library's card and page (connectors spec, Catalog and connector
    /// manifests): who makes it ("by …"), its category chip, what Magnus can
    /// do with it, each scope in plain words, and what it can and cannot do
    /// here. `letters` and `tint` draw its tile until the provider's own mark
    /// is in its listing.
    pub publisher: String,
    pub category: String,
    pub abilities: Vec<String>,
    pub asks: Vec<String>,
    pub can: String,
    pub cannot: String,
    pub letters: String,
    pub tint: String,
    /// `remote` or `brokered`.
    pub kind: String,
    /// Remote: the provider's MCP address. Brokered: the server image's
    /// digest, `sha256:<hex>` (connectors spec, the manifest's fields).
    pub endpoint: String,
    /// Brokered: the provider API hosts its microVM may reach, and the
    /// provider's authorization server people sign in at.
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default)]
    pub issuer: Option<String>,
    pub audience: String,
    pub label: String,
    /// `dcr` (Magnus registers itself) or `preregistered` (Alpha Vector's
    /// own client with `provider`).
    pub registration: String,
    #[serde(default)]
    pub provider: Option<String>,
    /// Exactly the access asked for at sign-in.
    #[serde(default)]
    pub scopes: Vec<String>,
    /// Every tool's class.
    #[serde(default)]
    pub tools: BTreeMap<String, String>,
    /// What each tool reaches, where declared (approvals spec, The approval
    /// model): `self` for the person's own account. A tool left out is
    /// unknown, and asks as before.
    #[serde(default)]
    pub reach: BTreeMap<String, String>,
    /// The provider's page where a person removes Magnus's access, named
    /// when Disconnect could not revoke it there.
    #[serde(default)]
    pub remove_access: Option<String>,
    /// Whether the sign-in sends PKCE. On, unless the provider's client
    /// cannot take it (Slack's confidential client: opting in is
    /// irreversible and would make it public).
    #[serde(default = "pkce_on")]
    pub pkce: bool,
    /// A redirect the provider registered exactly. The desktop sends this
    /// at sign-in, and the token exchange repeats it. Absent: the desktop
    /// picks a free loopback port.
    #[serde(default)]
    pub redirect_uri: Option<String>,
    /// Where the appliance sends the browser on, when `redirect_uri` is the
    /// public https address. The desktop listens here.
    #[serde(default)]
    pub handoff: Option<String>,
    /// Where a tool's action goes, read from what it acts on.
    #[serde(default)]
    pub destinations: BTreeMap<String, crate::config::Destination>,
    /// How the gate asks whether the person has mailed an address before.
    #[serde(default)]
    pub sent: Option<crate::config::SentSearch>,
}

fn pkce_on() -> bool {
    true
}

pub fn manifests() -> &'static [Manifest] {
    static M: OnceLock<Vec<Manifest>> = OnceLock::new();
    M.get_or_init(|| SOURCES.iter().map(|s| toml::from_str(s).expect("a catalog manifest parses (checked by a unit test)")).collect())
}

/// The Library's category chips, in their order.
pub const CATEGORIES: [&str; 5] = ["email", "files", "code", "work", "design"];

pub fn manifest(id: &str) -> Option<&'static Manifest> {
    manifests().iter().find(|m| m.id == id)
}

/// Alpha Vector's own client with a provider.
#[derive(Deserialize, Clone, Debug)]
pub struct Client {
    pub client_id: String,
    /// `desktop` or `confidential` (a secret at the token exchange), or
    /// `public` (none).
    pub kind: String,
}

impl Client {
    pub fn needs_secret(&self) -> bool {
        self.kind != "public"
    }
}

/// Alpha Vector's client with `provider`, if it has one.
pub fn client(provider: &str) -> Option<&'static Client> {
    static C: OnceLock<BTreeMap<String, Client>> = OnceLock::new();
    C.get_or_init(|| toml::from_str(include_str!("../../../catalog/registrations.toml")).expect("the catalog's registrations parse (checked by a unit test)")).get(provider)
}

/// The vault name of Alpha Vector's provisioned secret for a provider.
pub fn registration_name(provider: &str) -> String {
    format!("registration_{provider}")
}

/// A manifest as one of the organisation's apps. Its client ID is read
/// from the registration at sign-in, never kept here.
pub fn connector(m: &Manifest) -> Connector {
    Connector {
        name: m.id.clone(),
        transport: if m.kind == "brokered" { crate::brokered::BROKERED.into() } else { "mcp_http".into() },
        url: m.endpoint.clone(),
        credential: Some(crate::connectors::credential_name(&m.id)),
        audience: m.audience.clone(),
        label: m.label.clone(),
        access_groups: vec![],
        blocked_groups: vec![],
        idempotency: "none".into(),
        tools: m.tools.iter().map(|(t, risk)| (t.clone(), ToolClass { risk: risk.clone(), label: None, reach: m.reach.get(t).cloned() })).collect(),
        auth: Some(crate::connectors::OAUTH.into()),
        client_id: None,
        system_of_record: false,
        scopes: m.scopes.clone(),
        registration: m.provider.clone().filter(|_| m.registration == "preregistered"),
        title: Some(m.name.clone()),
        summary: Some(m.summary.clone()),
        catalog: true,
        hosts: m.hosts.clone(),
        issuer: m.issuer.clone(),
        pkce: m.pkce,
        redirect_uri: m.redirect_uri.clone(),
        handoff: m.handoff.clone(),
        destinations: m.destinations.clone(),
        sent: m.sent.clone(),
        watch: vec![],
    }
}

/// What is wrong with a manifest's kind and where it points, if anything:
/// a remote one names its MCP address and nothing a brokered one does; a
/// brokered one names its image by digest, the hosts it may reach and
/// where people sign in.
pub fn kind_problem(m: &Manifest) -> Option<String> {
    match m.kind.as_str() {
        "remote" if !m.endpoint.starts_with("https://") => Some(format!("{}: a remote connector's endpoint is its https address", m.id)),
        "remote" if !m.hosts.is_empty() || m.issuer.is_some() => Some(format!("{}: a remote connector names no hosts or issuer: its server says where to sign in", m.id)),
        "remote" => None,
        "brokered" => {
            if let Err(e) = crate::brokered::digest(&m.endpoint) {
                return Some(format!("{}: {e}", m.id));
            }
            if m.hosts.is_empty() || m.hosts.iter().any(|h| h.is_empty() || h.contains('/') || h.contains('*')) {
                return Some(format!("{}: a brokered connector names each provider host its microVM may reach, exactly", m.id));
            }
            match &m.issuer {
                Some(i) if i.starts_with("https://") => None,
                _ => Some(format!("{}: a brokered connector names the https address where people sign in", m.id)),
            }
        }
        k => Some(format!("{}: unknown kind {k}", m.id)),
    }
}

#[cfg(test)]
mod tests {
    /// Every manifest parses, names a known kind, registration, audience,
    /// label and tool class, and a preregistered one names its provider.
    #[test]
    fn every_manifest_is_well_formed() {
        let ms = super::manifests();
        assert!(!ms.is_empty());
        let mut ids = std::collections::HashSet::new();
        for m in ms {
            assert!(ids.insert(m.id.clone()), "{} twice", m.id);
            assert!(!m.id.contains("__") && m.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'), "{}", m.id);
            assert_eq!(super::kind_problem(m), None);
            assert!(["dcr", "preregistered"].contains(&m.registration.as_str()), "{}", m.id);
            assert_eq!(m.registration == "preregistered", m.provider.is_some(), "{}", m.id);
            // Where the person removes Magnus when the app cannot end a sign-in itself.
            assert!(m.remove_access.as_deref().is_some_and(|u| u.starts_with("https://")), "{}: no page where the person removes Magnus", m.id);
            assert!(crate::tools::Audience::parse(&m.audience).is_some(), "{}", m.id);
            assert!(crate::tools::Label::parse(&m.label).is_some(), "{}", m.id);
            for (t, risk) in &m.tools {
                assert!(["read", "reversible_write", "irreversible", "financial", "prohibited"].contains(&risk.as_str()), "{}: {t} is {risk}", m.id);
            }
            // A reach is declared only for a tool the manifest classes, and fits its class.
            for (t, reach) in &m.reach {
                let risk = m.tools.get(t).unwrap_or_else(|| panic!("{}: reach for {t}, which it does not class", m.id));
                assert!(crate::tools::reach_fits(reach, risk), "{}: {t} is {risk} and cannot reach {reach:?}", m.id);
            }
            for (t, d) in &m.destinations {
                assert_eq!(d.fault(t, |t| m.tools.get(t).cloned()), None, "{}", m.id);
                assert!(m.tools.contains_key(t), "{}: destinations for {t}, which it does not have", m.id);
            }
            if let Some(s) = &m.sent {
                assert_eq!(s.fault(|t| m.tools.get(t).cloned()), None, "{}", m.id);
            }
            // What the Library shows: a category chip, abilities, a plain line
            // for every scope asked for, and never a protocol word.
            assert!(super::CATEGORIES.contains(&m.category.as_str()), "{}: category {}", m.id, m.category);
            assert!(!m.publisher.is_empty() && !m.abilities.is_empty() && !m.can.is_empty() && !m.cannot.is_empty(), "{}: its listing is incomplete", m.id);
            assert_eq!(m.asks.len(), m.scopes.len(), "{}: one plain line for each scope asked for", m.id);
            assert!((1..=2).contains(&m.letters.chars().count()) && m.tint.starts_with('#') && m.tint.len() == 7, "{}: its tile", m.id);
            let shown = [&m.summary, &m.can, &m.cannot].into_iter().chain(&m.abilities).chain(&m.asks).map(|s| s.to_lowercase()).collect::<Vec<_>>();
            for word in ["mcp", "oauth", "scope", "token", "api", "port"] {
                assert!(shown.iter().all(|s| !s.split(|c: char| !c.is_alphanumeric()).any(|w| w == word)), "{}: protocol word {word:?} in what the Library shows", m.id);
            }
            // A preregistered connector is set up already: its provider's client is in the catalog.
            if let Some(p) = &m.provider {
                assert!(super::client(p).is_some(), "{}: no client for {p} in catalog/registrations.toml", m.id);
            }
            match (&m.redirect_uri, &m.handoff) {
                (None, None) => {}
                (Some(u), None) => assert!(crate::connectors::loopback(u).is_ok(), "{}: its redirect is not a loopback address", m.id),
                (Some(u), Some(h)) => {
                    assert_eq!(u, crate::connectors::SLACK_OAUTH_CALLBACK, "{}: the https redirect", m.id);
                    assert_eq!(h, crate::connectors::SLACK_HANDOFF, "{}: the handoff", m.id);
                }
                (None, Some(_)) => panic!("{}: a handoff needs the registered redirect", m.id),
            }
        }
        for p in ["google", "microsoft", "github", "vercel", "slack", "sentry"] {
            let c = super::client(p).unwrap_or_else(|| panic!("{p}"));
            assert!(!c.client_id.is_empty() && ["desktop", "confidential", "public"].contains(&c.kind.as_str()), "{p}: {c:?}");
        }
    }

    /// A brokered manifest names its image by digest and the hosts it may
    /// reach, and points nowhere else; one that leaves either out, or names
    /// an address, is refused.
    #[test]
    fn a_brokered_manifest_names_its_image_and_hosts() {
        let base = |kind: &str, endpoint: &str, rest: &str| -> super::Manifest {
            toml::from_str(&format!(
                "id = \"x\"\nversion = \"1\"\nname = \"X\"\nsummary = \"s\"\npublisher = \"p\"\ncategory = \"email\"\nabilities = []\nasks = []\ncan = \"c\"\ncannot = \"c\"\nletters = \"X\"\ntint = \"#000000\"\naudience = \"private\"\nlabel = \"private+untrusted\"\nregistration = \"dcr\"\nkind = \"{kind}\"\nendpoint = \"{endpoint}\"\n{rest}"
            ))
            .unwrap()
        };
        let digest = format!("sha256:{}", "ab".repeat(32));
        let ok = base("brokered", &digest, "hosts = [\"gmail.googleapis.com\"]\nissuer = \"https://accounts.google.com\"");
        assert_eq!(super::kind_problem(&ok), None);
        let c = super::connector(&ok);
        assert_eq!((c.transport.as_str(), c.url.as_str(), c.hosts.as_slice()), ("brokered", digest.as_str(), &["gmail.googleapis.com".to_string()][..]));
        for (endpoint, rest) in [
            ("https://gmailmcp.googleapis.com/mcp/v1", "hosts = [\"gmail.googleapis.com\"]\nissuer = \"https://accounts.google.com\""),
            ("sha256:abc", "hosts = [\"gmail.googleapis.com\"]\nissuer = \"https://accounts.google.com\""),
            (digest.as_str(), "issuer = \"https://accounts.google.com\""),
            (digest.as_str(), "hosts = [\"*.googleapis.com\"]\nissuer = \"https://accounts.google.com\""),
            (digest.as_str(), "hosts = [\"gmail.googleapis.com\"]"),
        ] {
            assert!(super::kind_problem(&base("brokered", endpoint, rest)).is_some(), "{endpoint} {rest}");
        }
        assert!(super::kind_problem(&base("remote", "https://a.example/mcp", "hosts = [\"a.example\"]")).is_some());
    }
}
