//! Which vault entries are a person's own. The owner writes the server's
//! secrets (a mailbox password, a model provider's key) through
//! `PUT /v1/secrets/{name}`, and sees their names; a person's own are
//! written only by that person's own flows, never through that route.
//! Spec rev 145: "Each user has their own VM, their own key in the one
//! credential service"; connectors spec: each person signs in with their own
//! account "and no one else acts through it". An owner who could write one
//! could put their own account under someone's sign-in, and that person's
//! work would then flow into it.
use crate::kernel::Kernel;

/// Prefixes of names that hold one person's own secret: their card and
/// Link wallet, their browser identities and profiles' keys, and the
/// credentials stored for their computer.
const PERSONAL_PREFIXES: &[&str] = &["payments/", "browser-profile/", "identity/", "surrogate/"];

pub(crate) const REFUSED: &str = "that is a person's own secret: only they set it, through their own sign-in or settings";

/// Whether `name` is someone's own: a prefix above, a person's sign-in to an
/// app (`connector_<app>__<person>`, `connectors::connection_name`), or the
/// token of a connector a person built for themselves.
pub(crate) fn personal(k: &Kernel, name: &str) -> bool {
    PERSONAL_PREFIXES.iter().any(|p| name.starts_with(p))
        || (name.starts_with("connector_") && name.contains("__"))
        || k.read(|s| s.connectors.values().any(|c| c.owner.is_some() && c.credential.as_deref() == Some(name)))
}

/// The names the owner may see: the server's own.
pub(crate) fn server_names(k: &Kernel, names: Vec<String>) -> Vec<String> {
    names.into_iter().filter(|n| !personal(k, n)).collect()
}

#[cfg(test)]
mod tests {
    use crate::admin_app::tests::server_with;
    use crate::events::Ev;
    use axum::Router;
    use reqwest::Method;
    use serde_json::json;

    /// sam owns the server, mo is a member with a sign-in, a card and an app
    /// of their own making.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_owner_neither_writes_nor_sees_a_persons_own_secrets_or_apps() {
        let s = server_with("", |_| Router::new()).await;
        let mut own = crate::catalog::connector(&crate::catalog::manifests()[0]);
        own.name = "mo_tool_1a2b3c4d".into();
        own.owner = Some("mo".into());
        own.auth = None;
        own.registration = None;
        own.credential = Some(crate::connectors::credential_name(&own.name));
        s.k.emit("person:mo", Ev::ConnectorAdded { connector: own.clone() }).await.unwrap();
        let mine = ["connector_gmail__mo", "payments/card/mo", "payments/link/mo", "connector_mo_tool_1a2b3c4d"];
        for n in mine {
            s.k.vault.put(n, "mo's").unwrap();
        }
        let sam = s.token("sam").await;
        // Stepped up, as an admin change needs, so a refusal is this rule's.
        assert_eq!(s.post(&sam, "/v1/admin/step-up", json!({ "password": "a-long-admin-password" })).await.0, 200);
        assert_eq!(s.call(&sam, Method::PUT, "/v1/secrets/mail_token", Some(json!({ "value": "server's" }))).await.0, 204, "the owner could not set a server secret");
        for n in mine {
            let (st, v) = s.call(&sam, Method::PUT, &format!("/v1/secrets/{}", n.replace('/', "%2F")), Some(json!({ "value": "sam's" }))).await;
            assert_eq!(st, 403, "the owner wrote {n}: {v}");
            assert_eq!(s.k.vault.get(n).unwrap().map(|x| x.expose().to_string()).as_deref(), Some("mo's"), "{n} changed");
        }
        for n in ["mail_token", "oa_credential", "connector_crm"] {
            assert!(!super::personal(&s.k, n), "the server's {n} read as a person's");
        }
        let names = s.get(&sam, "/v1/secrets").await.1["names"].to_string();
        assert!(!mine.iter().any(|n| names.contains(n)) && names.contains("mail_token"), "{names}");
        // The app mo built is theirs: not the organisation's to remove,
        // reclass, list with its definition, or give a token.
        assert_eq!(s.call(&sam, Method::DELETE, &format!("/v1/connectors/{}", own.name), None).await.0, 404, "the owner removed mo's own app");
        assert_eq!(s.call(&sam, Method::PUT, &format!("/v1/connectors/{}/tools/x", own.name), Some(json!({ "risk": "read" }))).await.0, 404, "the owner reclassed mo's tool");
        assert!(!s.k.admin_connectors().iter().any(|c| c["name"] == json!(own.name)), "an admin lists mo's own app");
        assert!(s.k.set_connector_credential("person:sam", &own.name, "sam's").await.is_err(), "an admin set mo's app's token");
        assert!(s.k.revoke_connector_credential("person:sam", &own.name).await.is_err(), "an admin revoked mo's app's token");
        assert!(s.k.connector_settings("person:sam", &own.name, Some(false), None, None).await.is_err(), "an admin turned mo's app off");
        assert_eq!(s.k.vault.get("connector_mo_tool_1a2b3c4d").unwrap().map(|x| x.expose().to_string()).as_deref(), Some("mo's"));
        s.done().await;
    }
}
