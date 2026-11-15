//! The bodies the apps' routes take, each of a fixed shape, so one that
//! does not fit is refused (`extract::Body`) instead of read as empty.

use serde::Deserialize;

/// A provider's client secret, provisioned by the deployment.
#[derive(Deserialize)]
pub(super) struct Registration {
    pub client_secret: String,
}

/// A person's sign-in to an app: where the provider sends them back, the
/// app's own page when they name none.
#[derive(Deserialize)]
pub(super) struct SignIn {
    #[serde(default)]
    pub redirect_uri: String,
}

/// The provider's answer to a sign-in, as the app passes it on.
#[derive(Deserialize)]
pub(super) struct SignInComplete {
    pub state: Option<String>,
    pub code: Option<String>,
    pub iss: Option<String>,
}

/// An app added by its MCP address. Only the address is required: the
/// name comes from the app when none is given, and an audience or label
/// left out is refused with what it must be. A key, token or client id is
/// never taken; they are read only to be refused.
#[derive(Deserialize)]
pub(crate) struct AddConnector {
    #[serde(default)]
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub audience: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub client_id: Option<serde_json::Value>,
    #[serde(default)]
    pub access_groups: Vec<String>,
    #[serde(default)]
    pub blocked_groups: Vec<String>,
    #[serde(default)]
    pub system_of_record: bool,
}

/// One tool's class: a risk, or null to stop offering it.
#[derive(Deserialize)]
pub(super) struct Classify {
    pub risk: Option<String>,
}
