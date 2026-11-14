//! Where a connector is signed in to, as its transport says: the one rule
//! the direct sign-in and the relay's both follow.

use super::{asks_sign_in, discover, server_metadata};
use crate::config::Connector;
use anyhow::{bail, Result};
use serde_json::Value;

/// The authorization server a sign-in goes to and what it asks there.
pub(crate) struct SignInTarget {
    /// The server's metadata (RFC 8414).
    pub asm: Value,
    /// The scopes the app's protected-resource metadata names.
    pub scopes: Vec<String>,
    pub issuer: String,
    /// The resource indicator the token is for (RFC 8707); empty for none.
    pub resource: String,
}

/// A connector signed in to at its provider (a brokered one) signs in at
/// the authorization server its manifest names, with no resource indicator
/// (RFC 8707 names an MCP server a token is for; the provider's API takes
/// the plain OAuth 2.0 request). A remote app signs in where its MCP
/// server's metadata says, for that server.
pub(crate) async fn sign_in_target(c: &Connector) -> Result<SignInTarget> {
    if c.kind().signs_in_at_provider() {
        let Some(issuer) = &c.issuer else { bail!("{} names no authorization server to sign in at", c.name) };
        return Ok(SignInTarget { asm: server_metadata(issuer).await?, scopes: vec![], issuer: issuer.clone(), resource: String::new() });
    }
    let meta = asks_sign_in(&c.url).await?.unwrap_or(None);
    let (asm, scopes, issuer) = discover(&c.url, meta).await?;
    Ok(SignInTarget { asm, scopes, issuer, resource: c.url.clone() })
}

#[cfg(test)]
mod tests {
    use super::sign_in_target;
    use crate::config::Connector;

    fn connector(transport: &str, issuer: Option<&str>) -> Connector {
        serde_json::from_value(
            serde_json::json!({ "name": "mail", "transport": transport, "url": "http://127.0.0.1:9/mcp", "issuer": issuer, "audience": "private", "label": "private" }),
        )
        .unwrap()
    }

    /// A brokered connector with no authorization server of its own never
    /// falls back to asking its url (an image digest) where to sign in.
    #[tokio::test]
    async fn a_provider_sign_in_names_its_server_or_is_refused() {
        let e = sign_in_target(&connector("brokered", None)).await.err().map(|e| format!("{e:#}"));
        assert_eq!(e.as_deref(), Some("mail names no authorization server to sign in at"));
        // A remote app asks its MCP server, here one that is not there.
        let e = sign_in_target(&connector("mcp_http", None)).await.err().map(|e| format!("{e:#}")).unwrap_or_default();
        assert!(!e.contains("names no authorization server"), "{e}");
    }
}
