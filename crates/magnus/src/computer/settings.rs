//! `[computer]` in the server's config.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// `[computer]`: where computerd listens.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub socket: PathBuf,
    /// The connected app that holds logins. It answers `get_login` with
    /// `{ username, password }`, and the kernel fills them in itself.
    #[serde(default)]
    pub password_manager: Option<String>,
    /// Package registries, for downloads only. `*.name` is any subdomain.
    #[serde(default = "default_registries")]
    pub registries: Vec<String>,
    /// For a test's fake sites: names answered here, in order, with the last
    /// answer repeating. These are taken as public.
    #[serde(default)]
    pub public_hosts: BTreeMap<String, Vec<String>>,
    /// The same, but the private-address check still applies.
    #[serde(default)]
    pub hosts: BTreeMap<String, Vec<String>>,
    /// A PEM file of certificate authorities the egress proxy trusts beside
    /// the public ones, for a company's own sites.
    #[serde(default)]
    pub upstream_ca: Option<std::path::PathBuf>,
    /// A computer unused this long hibernates; 0 keeps it running.
    #[serde(default = "default_idle_hibernate_minutes")]
    pub idle_hibernate_minutes: u64,
    /// A computer running this long without a stop hibernates, busy or not,
    /// unless a person holds it; 0 sets no limit.
    #[serde(default = "default_max_run_minutes")]
    pub max_run_minutes: u64,
}

fn default_max_run_minutes() -> u64 {
    24 * 60
}

fn default_idle_hibernate_minutes() -> u64 {
    30
}

fn default_registries() -> Vec<String> {
    ["pypi.org", "files.pythonhosted.org", "registry.npmjs.org", "deb.debian.org", "security.debian.org", "crates.io", "static.crates.io", "index.crates.io"]
        .map(String::from)
        .to_vec()
}
