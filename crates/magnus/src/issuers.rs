//! Whether an authorization server is the one a sign-in went to (RFC 8414
//! §3.3, RFC 9207). An issuer is that one when it is the same address.
//! Microsoft's multi-tenant authorities are the one exception: one address
//! signs in people from many organisations. Their metadata names the issuer
//! `https://login.microsoftonline.com/{tenantid}/v2.0`, and each sign-in
//! answers for the person's own organisation, with its tenant ID in that
//! place (Microsoft identity platform, "OpenID Connect on the Microsoft
//! identity platform", the `issuer` value). Without this a catalog
//! connector at `organizations` could not find where to sign in.

/// Microsoft's sign-in host.
const MICROSOFT: &str = "https://login.microsoftonline.com/";
/// The authorities that sign in people from any organisation. `consumers`
/// is one tenant, whose issuer is its own ID, so it is not here.
const MULTI_TENANT: [&str; 2] = ["organizations", "common"];

/// Whether `named` (in metadata or an answer) is the issuer `asked` for.
pub fn same(named: &str, asked: &str) -> bool {
    let (named, asked) = (named.trim_end_matches('/'), asked.trim_end_matches('/'));
    named == asked || microsoft_tenant(named, asked)
}

/// `asked` is a Microsoft multi-tenant authority, and `named` is its
/// templated issuer, or one organisation's under it.
fn microsoft_tenant(named: &str, asked: &str) -> bool {
    let tenant = |s: &str| s.strip_prefix(MICROSOFT).and_then(|r| r.strip_suffix("/v2.0")).map(str::to_string);
    let (Some(n), Some(a)) = (tenant(named), tenant(asked)) else { return false };
    MULTI_TENANT.contains(&a.as_str()) && (n == "{tenantid}" || is_guid(&n))
}

/// A tenant ID: a GUID, 8-4-4-4-12 hex digits.
fn is_guid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.iter().map(|p| p.len()).eq([8, 4, 4, 4, 12]) && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::same;

    const ORGS: &str = "https://login.microsoftonline.com/organizations/v2.0";

    #[test]
    fn an_issuer_is_the_same_address() {
        assert!(same("https://accounts.google.com", "https://accounts.google.com/"));
        assert!(!same("https://accounts.google.com", "https://accounts.example.com"));
    }

    /// Microsoft's `organizations` metadata names `{tenantid}`, and a
    /// sign-in answers for one organisation's tenant: both are it.
    #[test]
    fn microsoft_s_multi_tenant_issuer_is_any_organisation_s() {
        assert!(same("https://login.microsoftonline.com/{tenantid}/v2.0", ORGS));
        assert!(same("https://login.microsoftonline.com/f1524b07-679e-443c-838b-594fb9e8576a/v2.0", ORGS));
        assert!(same("https://login.microsoftonline.com/{tenantid}/v2.0", "https://login.microsoftonline.com/common/v2.0"));
        // Not another host, version, or a tenant that is not an ID.
        for other in [
            "https://login.example.com/{tenantid}/v2.0",
            "https://login.microsoftonline.com/{tenantid}",
            "https://login.microsoftonline.com/evil/v2.0",
            "https://login.microsoftonline.com/f1524b07-679e-443c-838b-594fb9e8576/v2.0",
        ] {
            assert!(!same(other, ORGS), "{other}");
        }
        // One tenant's authority is that tenant alone.
        let one = "https://login.microsoftonline.com/f1524b07-679e-443c-838b-594fb9e8576a/v2.0";
        assert!(!same("https://login.microsoftonline.com/{tenantid}/v2.0", one));
        assert!(!same("https://login.microsoftonline.com/00000000-0000-0000-0000-000000000000/v2.0", one));
    }
}
