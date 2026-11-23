//! The one way the server reaches an address someone else chose
//! (`decisions/2026-11-23-one-egress-client.md`): an added app's MCP
//! address, every address its sign-in names (its protected-resource
//! metadata, authorization server, registration, authorization, token and
//! revocation endpoints), a generated connector's base, and a web fetch.
//!
//! Every request resolves its host once, refuses it when any address is
//! private (`net_guard`), unless the server allows private networks
//! (`[web] allow_private_networks`, for tests and company setups), and
//! connects only to the addresses checked, so DNS cannot answer differently
//! between the check and the connection. No client follows a redirect by
//! itself: `get` follows one hop at a time, each checked like the first,
//! and `send` follows none (a POST's protocol needs none, and a 307 would
//! replay a token or code to wherever it points).
//!
//! `ci/egress-check.sh` keeps this the only place a client is built for
//! such an address: anywhere else, a client reaches a fixed address the
//! operator configured.

use anyhow::{anyhow, Context, Result};
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

/// Redirect hops a `get` follows, each checked like the first request.
pub(crate) const MAX_REDIRECT_HOPS: usize = 5;

/// The addresses `host` reaches on `port`, refused when any is private, so a
/// caller connects to an address that was checked and DNS cannot answer
/// differently between the check and the connection (no rebinding). An
/// address literal is that address (an IPv6 one may keep its brackets, which
/// no resolver takes); a name is resolved once. Every answer is checked, or
/// a mix of one public and one private would pass.
pub(crate) async fn public_addrs(host: &str, port: u16) -> Result<Vec<SocketAddr>> {
    checked_addrs(host, port, |ip| !crate::net_guard::is_non_public_ip(ip)).await
}

async fn checked_addrs(host: &str, port: u16, public: impl Fn(IpAddr) -> bool) -> Result<Vec<SocketAddr>> {
    let bare = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host);
    let addrs: Vec<SocketAddr> = match bare.parse::<IpAddr>() {
        Ok(ip) => vec![(ip, port).into()],
        Err(_) => tokio::net::lookup_host((bare, port)).await?.collect(),
    };
    anyhow::ensure!(!addrs.is_empty(), "{host} does not resolve");
    for a in &addrs {
        anyhow::ensure!(public(a.ip()), "{host} resolves to a private address; reaching it is not allowed");
    }
    Ok(addrs)
}

/// How the server reaches addresses someone else chose: whether private
/// networks are allowed, and how long a connection and a whole request
/// may take.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Egress {
    allow_private: bool,
    connect_wait: Duration,
    total_wait: Duration,
    /// A test's stand-in for a public server: this one loopback address
    /// counts as public, so a redirect from it can be tried.
    #[cfg(test)]
    stand_in: Option<IpAddr>,
}

impl Egress {
    pub(crate) const fn new(allow_private: bool, connect_wait: Duration, total_wait: Duration) -> Egress {
        Egress {
            allow_private,
            connect_wait,
            total_wait,
            #[cfg(test)]
            stand_in: None,
        }
    }

    /// Private networks refused, but `ip` taken as a public server's.
    #[cfg(test)]
    pub(crate) fn with_stand_in(ip: IpAddr, wait: Duration) -> Egress {
        Egress { stand_in: Some(ip), ..Egress::new(false, wait, wait) }
    }

    fn public(&self, ip: IpAddr) -> bool {
        #[cfg(test)]
        if self.stand_in == Some(ip) {
            return true;
        }
        !crate::net_guard::is_non_public_ip(ip)
    }

    /// The same, with another limit on a whole request.
    pub(crate) const fn waiting(self, total_wait: Duration) -> Egress {
        Egress { total_wait, ..self }
    }

    /// A client for `url`'s host alone: at addresses checked public and
    /// pinned (unless private networks are allowed), following no redirect.
    /// Only http and https are reached.
    pub(crate) async fn client_for(&self, url: &url::Url) -> Result<reqwest::Client> {
        anyhow::ensure!(matches!(url.scheme(), "http" | "https"), "only http and https addresses can be reached");
        let host = url.host_str().ok_or_else(|| anyhow!("the address has no host"))?;
        let builder = reqwest::Client::builder().connect_timeout(self.connect_wait).timeout(self.total_wait).redirect(reqwest::redirect::Policy::none());
        let builder = if self.allow_private {
            builder
        } else {
            let addrs = checked_addrs(host, url.port_or_known_default().unwrap_or(443), |ip| self.public(ip)).await?;
            builder.resolve_to_addrs(host.trim_start_matches('[').trim_end_matches(']'), &addrs)
        };
        builder.build().context("an HTTP client")
    }

    /// One request to `url`, as `build` makes it, following no redirect: a
    /// 3xx comes back as it is.
    pub(crate) async fn send(&self, method: reqwest::Method, url: &str, build: impl Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder) -> Result<reqwest::Response> {
        let url = url::Url::parse(url).with_context(|| format!("{url} is not an address"))?;
        let client = self.client_for(&url).await?;
        Ok(build(client.request(method, url)).send().await?)
    }

    /// A GET to `url`, following up to `MAX_REDIRECT_HOPS` redirects, each
    /// hop's address checked and pinned like the first.
    pub(crate) async fn get(&self, url: &str, build: impl Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder) -> Result<reqwest::Response> {
        let mut url = url::Url::parse(url).with_context(|| format!("{url} is not an address"))?;
        for _ in 0..=MAX_REDIRECT_HOPS {
            let client = self.client_for(&url).await?;
            let resp = build(client.get(url.clone())).send().await?;
            if !resp.status().is_redirection() {
                return Ok(resp);
            }
            let to = resp.headers().get(reqwest::header::LOCATION).and_then(|v| v.to_str().ok()).ok_or_else(|| anyhow!("a redirect without a location"))?;
            url = url.join(to).map_err(|e| anyhow!("a bad redirect: {e}"))?;
        }
        Err(anyhow!("too many redirects"))
    }
}

/// A client for an address the operator wrote in the config (the relay,
/// the owner's search service), never one someone else chose. It follows
/// no redirect either.
pub(crate) fn configured(connect_wait: Duration, total_wait: Duration) -> Result<reqwest::Client> {
    reqwest::Client::builder().connect_timeout(connect_wait).timeout(total_wait).redirect(reqwest::redirect::Policy::none()).build().context("an HTTP client")
}

#[cfg(test)]
#[path = "egress_client_tests.rs"]
pub(crate) mod tests;
