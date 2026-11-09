//! The identity interface: how people prove who they are, through whichever
//! identity provider the appliance uses. An OpenID Connect provider
//! (`oidc.rs`; Keycloak for one) is the one built. People type their
//! username and password in the app, and the appliance checks them with the
//! provider; a passkey signs in in the person's browser instead. SAML is to
//! sign in the browser way, and SCIM to feed the same people and groups
//! (docs/proposals/accounts.md). Every decision about a person still
//! reads `Kernel::person`, whichever provider signed them in.

use anyhow::Result;

/// Groups and roles the provider gave, where the owner named where they
/// are; None where not.
#[derive(Debug, Default, Clone)]
pub struct Rights {
    pub groups: Option<Vec<String>>,
    pub roles: Option<Vec<String>>,
    /// The provider's lasting identifier for the account (OIDC `sub`),
    /// which a username reused by a new account does not carry over.
    pub subject: Option<String>,
}

/// Where to send the person's browser, and for how long the sign-in waits.
pub struct Begun {
    pub state: String,
    pub authorize_url: String,
    pub expires_in: i64,
}

pub enum Polled {
    /// Not finished yet; ask again later.
    Pending,
    /// Signed in: who, whether in the app, and their rights.
    Done { person: String, app: bool, rights: Rights },
    /// Over: declined, expired or refused, with a message for the person.
    Failed(String),
    /// The password may be right, and the person also uses a code from an
    /// authenticator app: ask for it and try again.
    NeedsCode,
    /// The username, password or code is wrong, with a message for the person.
    Refused(String),
    /// Only the provider's own page can sign this person in: their account
    /// still needs setting up, or the provider knows no such username (it
    /// may be one the page takes, such as an email address). The app opens
    /// it in their browser, with this message.
    UseBrowser(String),
}

#[async_trait::async_trait]
pub trait Identity: Send + Sync {
    /// What the sign-in button names.
    fn label(&self) -> String;
    /// Begin a sign-in in the person's own browser, which comes back to
    /// `redirect_uri`, a loopback address on their computer. `login_hint`
    /// fills in the username they already typed.
    async fn begin(&self, redirect_uri: &str, login_hint: Option<&str>, app: bool) -> Result<Begun>;
    /// Finish it with what came back: the code, the state it answers, and
    /// who sent it back, if the provider says.
    async fn finish(&self, state: &str, code: &str, iss: Option<&str>) -> Result<Polled>;
    /// Check a username and password the person typed in the app, with the
    /// code from their authenticator app when they use one. Never kept.
    async fn password(&self, username: &str, password: &str, code: Option<&str>, app: bool) -> Result<Polled>;
    /// Whether the provider still has this person, turned on: the account
    /// they are bound to (`sub`), when they are. Their sessions here end
    /// once it does not.
    async fn active(&self, username: &str, sub: Option<&str>) -> Result<bool>;
    /// Whether the organisation's directory (the realm's people) holds this
    /// address: a recipient-provenance source at the send gate (#211).
    async fn in_directory(&self, address: &str) -> Result<bool>;
}
