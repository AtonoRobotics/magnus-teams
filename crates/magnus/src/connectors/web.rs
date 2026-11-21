//! Connecting an app from a web browser
//! (`decisions/2026-11-21-connect-on-the-web.md`).
//!
//! The desktop app catches a sign-in's answer on a loopback address on the
//! person's computer. A browser has none, so the provider sends it back to
//! this server's own address, `<public_url>/connected`, which serves the
//! app; the app, in the person's own session, finishes the sign-in as the
//! desktop does. Only a provider that takes an address Magnus names can do
//! this: one Magnus registers itself with (dynamic client registration).
//! A client registered once with a fixed list of addresses (Google's
//! desktop client, Asana, Slack, Vercel, GitHub, Microsoft) lists only the
//! desktop's, so those apps are still connected from the desktop app.
use crate::config::Connector;
use crate::kernel::Kernel;
use crate::public_url::PublicUrl;
use axum::extract::Path;
use axum::http::{header, HeaderValue};
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

/// This server's return address for a browser's sign-in to `c`, if the
/// provider will send a browser there.
pub(crate) fn return_for(public: Option<&PublicUrl>, c: &Connector) -> Option<String> {
    public.filter(|_| takes_a_named_return(c)).map(|u| u.join(RETURN_PATH))
}

impl Kernel {
    /// Whether a browser can connect `c` here: its provider takes an
    /// address Magnus names, and it does not sign in through the relay,
    /// whose own callback is the redirect.
    pub(crate) fn web_sign_in(&self, c: &Connector) -> bool {
        takes_a_named_return(c) && self.via_relay(c).is_none()
    }

    /// The address a browser's sign-in to `c` comes back to here, if one
    /// can: on the server's public address.
    pub(crate) fn web_return(&self, c: &Connector) -> Option<String> {
        return_for(self.cfg.appliance.public_url.as_ref(), c).filter(|_| self.web_sign_in(c))
    }

    /// Whether `redirect_uri` is this server's return address for `c`.
    pub(crate) fn is_web_return(&self, c: &Connector, redirect_uri: &str) -> bool {
        self.web_return(c).is_some_and(|r| r == redirect_uri)
    }
}

/// `/connected`: the app, which finishes the sign-in the provider sent the
/// browser back with. The address carries the code, so no page it loads is
/// told where it came from.
pub(crate) async fn connected() -> Response {
    let mut r = crate::ui::assets::ui_file(Path("index.html".into())).await;
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
    /// a client id in the config does not, and with no public address
    /// nothing does.
    #[test]
    fn only_a_provider_that_takes_a_named_address_returns_to_the_server() {
        let public = PublicUrl::parse("https://teams.pyrallon.ai/").unwrap();
        assert_eq!(return_for(Some(&public), &app("")).as_deref(), Some("https://teams.pyrallon.ai/connected"));
        assert_eq!(return_for(None, &app("")), None);
        for fixed in ["redirect_uri = \"http://127.0.0.1:17322/callback\"", "registration = \"google\"", "client_id = \"abc\""] {
            assert_eq!(return_for(Some(&public), &app(fixed)), None, "{fixed}");
        }
    }

    /// The page is the app, and it tells nothing it loads the address with
    /// the code in it.
    #[tokio::test]
    async fn the_return_page_is_the_app_and_sends_no_referrer() {
        let r = connected().await;
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers()[header::REFERRER_POLICY], "no-referrer");
        assert!(r.headers()[header::CONTENT_TYPE].to_str().unwrap().starts_with("text/html"));
    }
}
