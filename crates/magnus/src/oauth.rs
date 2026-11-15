//! The pieces of an OAuth sign-in every one of Magnus's shares: the random
//! values, the PKCE challenge (RFC 7636) and the loopback redirect check
//! (RFC 8252). The apps' sign-in (`connectors`), the relay's and the
//! server's own identity provider (`oidc`) use them.

use anyhow::{bail, Context, Result};
use base64::Engine;

pub(crate) fn random() -> String {
    use rand::RngCore;
    let mut b = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut b);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

pub(crate) fn challenge(verifier: &str) -> String {
    use sha2::Digest;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier.as_bytes()))
}

/// RFC 8252 loopback redirects only: the code goes to the person's own
/// computer, never to another host.
pub(crate) fn loopback(uri: &str) -> Result<()> {
    let u = url::Url::parse(uri).context("the redirect address is not a URL")?;
    let host = u.host_str().unwrap_or_default();
    if u.scheme() != "http" || !matches!(host, "127.0.0.1" | "[::1]" | "localhost") || u.port().is_none() {
        bail!("the redirect address must be http://127.0.0.1:<port>/... on the person's computer");
    }
    Ok(())
}
