//! Shared SSRF / non-public IP blocklist and multi-tenant public suffix guard.
//!
//! Keeps Rust's `web_fetch` and `computer/egress` blocklists in parity with
//! `browser/destinations.js` so IPv6 transition ranges (NAT64, 6to4, Teredo,
//! IPv4-compatible/mapped), multicast, benchmarking, and reserved blocks are
//! refused consistently across all outbound paths.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Multi-tenant and public suffixes where granting `*.<host>` to an Identity
/// browser VM would expose unrelated tenants' subdomains.
const MULTI_TENANT_SUFFIXES: &[&str] = &[
    "github.io",
    "gitlab.io",
    "pages.dev",
    "workers.dev",
    "vercel.app",
    "netlify.app",
    "herokuapp.com",
    "appspot.com",
    "run.app",
    "web.app",
    "firebaseapp.com",
    "amazonaws.com",
    "cloudfront.net",
    "azurewebsites.net",
    "blob.core.windows.net",
    "storage.googleapis.com",
    "fly.dev",
    "onrender.com",
    "railway.app",
    "replit.dev",
    "ngrok.io",
    "ngrok-free.app",
    "trycloudflare.com",
    "blogspot.com",
    "wordpress.com",
    "tumblr.com",
    "notion.site",
    "webflow.io",
    "myshopify.com",
    "co.uk",
    "org.uk",
    "ac.uk",
    "gov.uk",
    "com.au",
    "net.au",
    "org.au",
    "co.jp",
    "co.nz",
    "com.br",
    "co.in",
];

/// Returns `true` if `ip` is private, loopback, link-local, multicast,
/// reserved, or an IPv6 transition address that must not be reached by
/// public web fetches or egress proxies.
pub(crate) fn is_non_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_non_public_v4(v4),
        IpAddr::V6(v6) => is_non_public_v6(v6),
    }
}

fn is_non_public_v4(v: Ipv4Addr) -> bool {
    let [a, b, c, _] = v.octets();
    a == 0 // 0.0.0.0/8 ("this network"; Linux delivers 0.x to localhost)
        || a == 10 // 10.0.0.0/8 (RFC 1918)
        || (a == 100 && (64..128).contains(&b)) // 100.64.0.0/10 (CGNAT / Tailscale)
        || a == 127 // 127.0.0.0/8 (loopback)
        || (a == 169 && b == 254) // 169.254.0.0/16 (link-local / metadata)
        || (a == 172 && (16..32).contains(&b)) // 172.16.0.0/12 (RFC 1918)
        || (a == 192 && b == 0 && c == 0) // 192.0.0.0/24 (IETF protocol assignments)
        || (a == 192 && b == 0 && c == 2) // 192.0.2.0/24 (TEST-NET-1)
        || (a == 192 && b == 168) // 192.168.0.0/16 (RFC 1918)
        || (a == 198 && (18..=19).contains(&b)) // 198.18.0.0/15 (benchmarking)
        || (a == 198 && b == 51 && c == 100) // 198.51.100.0/24 (TEST-NET-2)
        || (a == 203 && b == 0 && c == 113) // 203.0.113.0/24 (TEST-NET-3)
        || a >= 224 // 224.0.0.0/4 (multicast) + 240.0.0.0/4 (reserved & broadcast)
}

fn is_non_public_v6(v: Ipv6Addr) -> bool {
    if let Some(v4) = v.to_ipv4_mapped() {
        return is_non_public_v4(v4);
    }
    let s = v.segments();
    v.is_loopback()
        || v.is_unspecified()
        // ::/96 deprecated IPv4-compatible
        || (s[0] == 0 && s[1] == 0 && s[2] == 0 && s[3] == 0 && s[4] == 0 && s[5] == 0)
        // 64:ff9b::/96 (well-known NAT64) and 64:ff9b:1::/48 (local-use NAT64)
        || (s[0] == 0x0064 && s[1] == 0xff9b && ((s[2] == 0 && s[3] == 0 && s[4] == 0 && s[5] == 0) || s[2] == 1))
        // 100::/64 (discard-only)
        || (s[0] == 0x0100 && s[1] == 0 && s[2] == 0 && s[3] == 0)
        // 2001::/32 (Teredo)
        || (s[0] == 0x2001 && s[1] == 0)
        // 2001:db8::/32 (documentation)
        || (s[0] == 0x2001 && s[1] == 0x0db8)
        // 2002::/16 (6to4)
        || s[0] == 0x2002
        // fc00::/7 (ULA)
        || (s[0] & 0xfe00) == 0xfc00
        // fe80::/10 (link-local)
        || (s[0] & 0xffc0) == 0xfe80
        // fec0::/10 (deprecated site-local)
        || (s[0] & 0xffc0) == 0xfec0
        // ff00::/8 (multicast)
        || (s[0] & 0xff00) == 0xff00
}

/// Returns `true` if `host` may safely receive a wildcard `*.<host>` grant in
/// an Identity VM. Single-label hosts, IP literals, and multi-tenant/public
/// suffixes return `false` so only exact-host access is granted.
pub(crate) fn may_grant_subdomains(host: &str) -> bool {
    let lower = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if lower.is_empty() || !lower.contains('.') || lower.parse::<IpAddr>().is_ok() {
        return false;
    }
    !MULTI_TENANT_SUFFIXES.iter().any(|suffix| lower == *suffix || (lower.ends_with(suffix) && lower.as_bytes().get(lower.len().saturating_sub(suffix.len() + 1)) == Some(&b'.')))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_all_non_public_v4_and_v6_ranges() {
        let blocked = [
            "0.0.0.0",
            "0.1.2.3",
            "10.0.0.1",
            "100.64.0.1",
            "100.127.255.254",
            "127.0.0.1",
            "169.254.169.254",
            "172.16.0.1",
            "192.0.0.1",
            "192.0.2.1",
            "192.168.1.1",
            "198.18.0.1",
            "198.19.255.254",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "240.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "::127.0.0.1",
            "::ffff:127.0.0.1",
            "::ffff:169.254.169.254",
            "64:ff9b::127.0.0.1",
            "64:ff9b:1::1",
            "100::1",
            "2001::1",
            "2001:db8::1",
            "2002:7f00:1::1",
            "fc00::1",
            "fd12::1",
            "fe80::1",
            "fec0::1",
            "ff02::1",
        ];
        for raw in blocked {
            let ip: IpAddr = raw.parse().unwrap();
            assert!(is_non_public_ip(ip), "{raw} should be blocked");
        }

        let allowed = ["1.1.1.1", "8.8.8.8", "93.184.216.34", "2606:2800:220:1:248:1893:25c8:1946", "::ffff:93.184.216.34"];
        for raw in allowed {
            let ip: IpAddr = raw.parse().unwrap();
            assert!(!is_non_public_ip(ip), "{raw} should be allowed");
        }
    }

    #[test]
    fn psl_blocks_wildcard_on_multi_tenant_suffixes() {
        assert!(!may_grant_subdomains("github.io"));
        assert!(!may_grant_subdomains("foo.github.io"));
        assert!(!may_grant_subdomains("s3.amazonaws.com"));
        assert!(!may_grant_subdomains("my-app.vercel.app"));
        assert!(!may_grant_subdomains("co.uk"));
        assert!(!may_grant_subdomains("1.2.3.4"));
        assert!(!may_grant_subdomains("localhost"));
        assert!(may_grant_subdomains("github.com"));
        assert!(may_grant_subdomains("linear.app"));
        assert!(may_grant_subdomains("example.com"));
    }
}
