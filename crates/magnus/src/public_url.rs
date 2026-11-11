//! The server's address on the internet (`[appliance] public_url`): where
//! providers send notifications (decisions/2026-11-10-push-subscriptions.md).
//! Checked when the config is read, so a server never starts with an
//! address no provider would call.

use serde::{Deserialize, Deserializer};
use std::net::IpAddr;

/// An `https://` address with a host on the internet, and nothing after
/// its path. Kept without a trailing slash, so a path joins straight on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicUrl(String);

impl PublicUrl {
    pub fn parse(s: &str) -> Result<PublicUrl, String> {
        let u = url::Url::parse(s).map_err(|e| format!("[appliance] public_url {s:?} is not an address: {e}"))?;
        if u.scheme() != "https" {
            return Err(format!("[appliance] public_url {s:?} must start with https://: providers send notifications only to https addresses"));
        }
        let host = u.host_str().unwrap_or_default();
        if host.is_empty() || !internet_host(host) {
            return Err(format!("[appliance] public_url {s:?} must name a host on the internet: no provider can reach {host:?}"));
        }
        if !u.username().is_empty() || u.password().is_some() || u.query().is_some() || u.fragment().is_some() {
            return Err(format!("[appliance] public_url {s:?} must be only the server's address: no user, password, query or fragment"));
        }
        Ok(PublicUrl(u.as_str().trim_end_matches('/').to_string()))
    }

    /// A path on this server (starting with `/`) as a full address.
    pub fn join(&self, path: &str) -> String {
        format!("{}{path}", self.0)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Not this machine and not a private network: a name with a dot, or a
/// public address.
fn internet_host(host: &str) -> bool {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    match bare.parse::<IpAddr>() {
        Ok(IpAddr::V4(a)) => !(a.is_loopback() || a.is_private() || a.is_link_local() || a.is_unspecified()),
        Ok(IpAddr::V6(a)) => !(a.is_loopback() || a.is_unspecified() || (a.segments()[0] & 0xfe00) == 0xfc00 || (a.segments()[0] & 0xffc0) == 0xfe80),
        Err(_) => bare.contains('.') && !bare.ends_with(".local") && !bare.ends_with(".localhost"),
    }
}

impl<'de> Deserialize<'de> for PublicUrl {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        PublicUrl::parse(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only an https address on the internet with nothing after its path is
    /// taken; a trailing slash is dropped so a push path joins cleanly.
    #[test]
    fn only_an_https_internet_address_is_taken() {
        let ok = PublicUrl::parse("https://teams.pyrallon.ai/").unwrap();
        assert_eq!(ok.as_str(), "https://teams.pyrallon.ai");
        assert_eq!(ok.join("/v1/push/a/b/c"), "https://teams.pyrallon.ai/v1/push/a/b/c");
        assert_eq!(PublicUrl::parse("https://example.com/magnus").unwrap().join("/v1/push/x"), "https://example.com/magnus/v1/push/x");
        assert_eq!(PublicUrl::parse("https://teams.pyrallon.ai:8443").unwrap().as_str(), "https://teams.pyrallon.ai:8443");
        for bad in [
            "http://teams.pyrallon.ai",
            "teams.pyrallon.ai",
            "https://localhost",
            "https://127.0.0.1",
            "https://192.168.0.10:7800",
            "https://10.1.2.3",
            "https://[::1]",
            "https://[fd00::1]",
            "https://strix",
            "https://box.local",
            "https://u:p@teams.pyrallon.ai",
            "https://teams.pyrallon.ai/?x=1",
            "https://teams.pyrallon.ai/#f",
            "",
        ] {
            assert!(PublicUrl::parse(bad).is_err(), "{bad:?} was taken");
        }
    }

    /// The config refuses a bad address when it is read, naming the setting.
    #[test]
    fn a_bad_address_stops_the_config_from_loading() {
        #[derive(Deserialize)]
        struct A {
            public_url: Option<PublicUrl>,
        }
        let e = toml::from_str::<A>("public_url = \"http://teams.pyrallon.ai\"").err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains("public_url") && e.contains("https"), "{e}");
        assert_eq!(toml::from_str::<A>("").unwrap().public_url, None);
        assert_eq!(toml::from_str::<A>("public_url = \"https://teams.pyrallon.ai\"").unwrap().public_url.unwrap().as_str(), "https://teams.pyrallon.ai");
    }
}
