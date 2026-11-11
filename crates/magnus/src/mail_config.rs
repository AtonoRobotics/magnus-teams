//! The `[mail]` section of the config: how the server sends account email.

use serde::Deserialize;

/// How the appliance sends email: host, port, encryption, and the sender.
/// Credentials never sit here: `username` names the login and `credential`
/// names the vault secret holding its password; both set, or neither (a
/// sink that takes mail unauthenticated, as tests run).
#[derive(Deserialize, Clone, Debug)]
pub struct Mail {
    pub host: String,
    #[serde(default = "mail_port")]
    pub port: u16,
    /// `starttls` (the default), `tls` (implicit, port 465), or `none`.
    #[serde(default = "mail_tls")]
    pub tls: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub credential: Option<String>,
    /// The envelope sender, for example `magnus@example.com`.
    pub from: String,
    /// The base account links point at, for example
    /// `https://magnus.example.com`. Unset: `[appliance] public_url`, else
    /// the public hostname when pairing is on, else no recovery mail goes
    /// out. The request's own
    /// host is never used: a forged Host header would mail a live token
    /// to the wrong domain (#541).
    #[serde(default)]
    pub link_base: Option<String>,
}

fn mail_port() -> u16 {
    587
}

fn mail_tls() -> String {
    "starttls".into()
}
