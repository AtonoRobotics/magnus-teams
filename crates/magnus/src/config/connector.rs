//! Connector, watched-read and tool-class settings in the configuration file.

use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize, serde::Serialize, Clone, Debug)]
pub struct Connector {
    /// Takes login codes and sign-in links out of what this app returns,
    /// as they are taken out of every mail app's (`login_codes::applies`).
    #[serde(default)]
    pub filter_login_codes: bool,
    /// Leaves them in, for a mail app too: the opt-out.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keep_login_codes: bool,
    #[serde(default)]
    pub owner: Option<String>, // The one person a generated connector belongs to.
    pub name: String,
    #[serde(default = "crate::transport::default_name")]
    pub transport: String,
    pub url: String,
    #[serde(default)]
    pub credential: Option<String>,
    pub audience: String,
    pub label: String,
    #[serde(default)]
    pub access_groups: Vec<String>,
    /// Groups blocked from it, whatever `access_groups` says (connectors spec).
    #[serde(default)]
    pub blocked_groups: Vec<String>,
    #[serde(default = "header")]
    pub idempotency: String,
    #[serde(default)]
    pub tools: BTreeMap<String, ToolClass>,
    /// `oauth`: the person signs in to the app (MCP authorization); the
    /// credential holds the tokens. Otherwise the credential is a bearer token.
    #[serde(default)]
    pub auth: Option<String>,
    /// A client registered with the app by hand, for an app that does not
    /// register clients itself.
    #[serde(default)]
    pub client_id: Option<String>,
    /// The company's record of what it holds (an ERP, a CRM): a knowledge
    /// entry whose quote is found in its answer is verified.
    #[serde(default)]
    pub system_of_record: bool,
    /// The access asked for at sign-in, when the app's manifest names it
    /// (else what the app's metadata offers).
    #[serde(default)]
    pub scopes: Vec<String>,
    /// The provider whose registration (Alpha Vector's client, set by the
    /// owner) signs people in to it.
    #[serde(default)]
    pub registration: Option<String>,
    /// Its name and one sentence on what Magnus can do with it, from its
    /// manifest.
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    /// From the catalog, not added by the owner.
    #[serde(default)]
    pub catalog: bool,
    /// A brokered connector (`transport = "brokered"`, `url` its server image's digest, `sha256:<hex>`): the provider API hosts
    /// its microVM may reach, `host` (port 443) or `host:port`, and the provider's authorization server people sign in at.
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default)]
    pub issuer: Option<String>,
    /// The sign-in sends PKCE (S256). Off only for a confidential client
    /// whose provider rejects a code challenge until the app opts in, and
    /// whose opt-in cannot be undone.
    #[serde(default = "pkce_on")]
    pub pkce: bool,
    /// The one redirect the provider registered. The token exchange repeats it.
    #[serde(default)]
    pub redirect_uri: Option<String>,
    /// The loopback address the desktop listens on when `redirect_uri` is the public https callback.
    #[serde(default)]
    pub handoff: Option<String>,
    /// Where a tool's action goes, read from what it acts on, by tool.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub destinations: BTreeMap<String, crate::gate_reads::Destination>,
    /// How the gate asks whether the person has mailed an address before
    /// (#210): a read tool called with a query naming the address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sent: Option<crate::gate_reads::SentSearch>,
    /// How the world is read back after a tool's action (spec rev 147), by tool.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub read_back: BTreeMap<String, crate::read_back::ReadBack>,
    /// Watched reads (#369): when any run's call of `tool` answers,
    /// the items at `items` (a JSON pointer into the answer, `""` for
    /// the answer itself) are diffed by their `id` field against what
    /// was last seen, and new ones wake a review for the person.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub watch: Vec<Watch>,
}

/// One watched read of an app (#369): `tool` names the remote tool,
/// `items` points at its answer's array, `id` names each item's
/// stable id. Items without an id are skipped, never waked on.
/// `poll_minutes`, when set, polls the read on schedule so items
/// nobody reads still wake (#406); unset watches diff only reads.
#[derive(Deserialize, serde::Serialize, Clone, Debug, PartialEq)]
pub struct Watch {
    pub tool: String,
    pub items: String,
    pub id: String,
    #[serde(default)]
    pub poll_minutes: Option<u64>,
    /// A poll's arguments; `{now}`, `{now-1d}` read as times (`watch_reads`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<serde_json::Value>,
}

fn pkce_on() -> bool {
    true
}

/// Apps are assumed not to deduplicate unless the owner says they honor an
/// `Idempotency-Key` header, so a crash never causes a blind resend.
fn header() -> String {
    "none".into()
}

#[derive(Deserialize, serde::Serialize, Clone, Debug)]
pub struct ToolClass {
    pub risk: String,
    /// Overrides the app's inbound label for this tool's results.
    #[serde(default)]
    pub label: Option<String>,
    /// What the tool reaches: `self` for the person's own account
    /// (`tools::REACH_SELF`); absent when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reach: Option<String>,
}
