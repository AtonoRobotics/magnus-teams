//! Connecting an app from a web browser
//! (`decisions/2026-11-21-connect-on-the-web.md`,
//! `decisions/2026-11-21-connect-registered-apps-on-the-web.md`).
//!
//! The desktop app catches a sign-in's answer on a loopback address on the
//! person's computer. A browser has none, so the provider sends it back to
//! this server's own address, `<public_url>/connected`, which serves the
//! app; the app, in the person's own session, finishes the sign-in as the
//! desktop does. A provider sends a browser there when Magnus registers its
//! own client for that address (dynamic client registration), or when
//! Alpha Vector's client with it lists that address
//! (`catalog/registrations.toml`, `web` and `returns`): Google's through a
//! web client beside the desktop one, Microsoft's with its secret, the
//! others through the same client. Any other app is connected from the
//! desktop app.
use crate::config::Connector;
use crate::kernel::Kernel;
use crate::public_url::PublicUrl;
use axum::http::{header, HeaderMap, HeaderValue};
use axum::response::Response;

/// Where a browser's sign-in comes back to on this server.
pub const RETURN_PATH: &str = "/connected";

/// Whether the app's provider sends the browser back to an address Magnus
/// names at sign-in: Magnus registers its own client there, for that
/// address. A fixed redirect, a client registered beforehand, or a client
/// id given in the config each mean a list of addresses set elsewhere.
pub(crate) fn takes_a_named_return(c: &Connector) -> bool {
    c.redirect_uri.is_none() && c.registration.is_none() && c.client_id.is_none()
}

/// The catalog registration a browser's sign-in to `c` goes through when
/// it comes back to `back`: the one its provider's client names for the
/// web, if that client lists `back` with the provider.
pub(crate) fn web_registration(c: &Connector, back: &str) -> Option<&'static str> {
    let web = crate::catalog::client(c.registration.as_deref()?)?.web.as_deref()?;
    crate::catalog::client(web)?.returns.iter().any(|r| r == back).then_some(web)
}

/// This server's return address for a browser's sign-in to `c`, if the
/// provider will send a browser there.
pub(crate) fn return_for(public: Option<&PublicUrl>, c: &Connector) -> Option<String> {
    let back = public?.join(RETURN_PATH);
    (takes_a_named_return(c) || web_registration(c, &back).is_some()).then_some(back)
}

impl Kernel {
    /// Whether a browser can connect `c` here: its provider takes an
    /// address Magnus names, or Alpha Vector's client lists this server's,
    /// and it does not sign in through the relay, whose own callback is the
    /// redirect.
    pub(crate) fn web_sign_in(&self, c: &Connector) -> bool {
        (takes_a_named_return(c) && self.via_relay(c).is_none()) || self.web_return(c).is_some()
    }

    /// The address a browser's sign-in to `c` comes back to here, if one
    /// can: on the server's public address, with the web client, where one
    /// is needed, complete on this server.
    pub(crate) fn web_return(&self, c: &Connector) -> Option<String> {
        let back = return_for(self.cfg.appliance.public_url.as_ref(), c).filter(|_| self.via_relay(c).is_none())?;
        match web_registration(c, &back) {
            Some(web) => crate::registrations::client(&self.vault, web).is_ok().then_some(back),
            None => Some(back),
        }
    }

    /// Whether `redirect_uri` is this server's return address for `c`.
    pub(crate) fn is_web_return(&self, c: &Connector, redirect_uri: &str) -> bool {
        self.web_return(c).is_some_and(|r| r == redirect_uri)
    }

    /// The catalog registration a sign-in to `c` coming back to
    /// `redirect_uri` goes through: the web client for this server's return
    /// address, else the connector's own.
    pub(crate) fn sign_in_registration<'a>(&self, c: &'a Connector, redirect_uri: &str) -> Option<&'a str> {
        match self.is_web_return(c, redirect_uri) {
            true => web_registration(c, redirect_uri).or(c.registration.as_deref()),
            false => c.registration.as_deref(),
        }
    }
}

/// `/connected`: the app, which finishes the sign-in the provider sent the
/// browser back with. The address carries the code, so no page it loads is
/// told where it came from.
pub(crate) async fn connected(headers: HeaderMap) -> Response {
    let mut r = crate::ui::assets::ui_index(headers).await;
    r.headers_mut().insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(extra: &str) -> Connector {
        let toml = format!(
            "name = \"x\"\nurl = \"https://x.example/mcp\"\ncredential = \"connector_x\"\nauth = \"oauth\"\naudience = \"private\"\nlabel = \"private+untrusted\"\n{extra}"
        );
        toml::from_str(&toml).unwrap()
    }

    /// An app Magnus registers itself with comes back to the server's own
    /// address; one with a fixed redirect, a client registered beforehand or
    /// a client id in the config does not, unless Alpha Vector's client
    /// lists that address; and with no public address nothing does.
    #[test]
    fn only_a_provider_that_takes_or_lists_the_address_returns_to_the_server() {
        let public = PublicUrl::parse("https://teams.pyrallon.ai/").unwrap();
        assert_eq!(return_for(Some(&public), &app("")).as_deref(), Some("https://teams.pyrallon.ai/connected"));
        assert_eq!(return_for(None, &app("")), None);
        for fixed in ["redirect_uri = \"http://127.0.0.1:17322/callback\"", "registration = \"sentry\"", "client_id = \"abc\""] {
            assert_eq!(return_for(Some(&public), &app(fixed)), None, "{fixed}");
        }
        // Registered clients that list the server's return address.
        for (provider, web) in [("google", "google_web"), ("microsoft", "microsoft_web"), ("github", "github"), ("slack", "slack"), ("asana", "asana"), ("vercel", "vercel")] {
            let c = app(&format!("registration = \"{provider}\"\nredirect_uri = \"http://127.0.0.1:17322/callback\""));
            assert_eq!(return_for(Some(&public), &c).as_deref(), Some("https://teams.pyrallon.ai/connected"), "{provider}");
            assert_eq!(web_registration(&c, "https://teams.pyrallon.ai/connected"), Some(web), "{provider}");
            // Another server's return address is not listed.
            let other = PublicUrl::parse("https://other.example/").unwrap();
            assert_eq!(return_for(Some(&other), &c), None, "{provider} on another server");
        }
    }

    /// A browser's sign-in through a web client is that client's own: the
    /// web clients sign in as a server's (a secret at the exchange), and the
    /// desktop's do not.
    #[test]
    fn the_web_clients_are_confidential_and_the_desktops_unchanged() {
        for web in ["google_web", "microsoft_web"] {
            let c = crate::catalog::client(web).unwrap();
            assert!(c.needs_secret() && c.web.is_none() && !c.returns.is_empty(), "{web}");
        }
        assert_eq!(crate::catalog::client("google").unwrap().kind, "desktop");
        assert_eq!(crate::catalog::client("microsoft").unwrap().kind, "public");
        assert_eq!(crate::catalog::client("microsoft_web").unwrap().client_id, crate::catalog::client("microsoft").unwrap().client_id);
        assert!(crate::catalog::client("sentry").unwrap().web.is_none(), "Sentry's client lists no web address");
    }

    /// The page is the app, and it tells nothing it loads the address with
    /// the code in it.
    #[tokio::test]
    async fn the_return_page_is_the_app_and_sends_no_referrer() {
        let r = connected(HeaderMap::new()).await;
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers()[header::REFERRER_POLICY], "no-referrer");
        assert!(r.headers()[header::CONTENT_TYPE].to_str().unwrap().starts_with("text/html"));
    }
}
