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
