//! Loading and validating the appliance configuration file.

use super::{Config, Person};

impl Config {
    pub fn load(path: &std::path::Path) -> anyhow::Result<Config> {
        let text = std::fs::read_to_string(path)?;
        let mut cfg: Config = toml::from_str(&text)?;
        cfg.path = path.canonicalize()?;
        cfg.outcomes.check()?;
        cfg.memory.check()?;
        if cfg.legal.is_set() {
            cfg.legal.check()?;
        } else {
            tracing::warn!("{}: no [legal] section, so the Terms of Use, usage-data choice and tester program are off until it is added", path.display());
        }
        if cfg.admin.is_some() {
            tracing::warn!("{}: [admin] configured the retired admin console and is ignored; administration is in the app", path.display());
        }
        cfg.images.as_ref().map(|i| i.check(&cfg.adapters)).transpose()?;
        if let Some(usd) = cfg.knowledge.embeddings.as_ref().and_then(|e| e.usd_per_mtok).filter(|u| !u.is_finite() || *u < 0.0) {
            anyhow::bail!("[knowledge.embeddings] usd_per_mtok is a number of US dollars, zero or more, not {usd}");
        }
        cfg.relay.as_ref().map(|r| r.check()).transpose()?;
        for f in &cfg.model.fallbacks {
            let Some((a, _m)) = f.split_once('/').map(|(a, m)| (a.trim(), m.trim())).filter(|(a, m)| !a.is_empty() && !m.is_empty()) else {
                anyhow::bail!("[model] fallbacks: {f:?} must be \"adapter/model\"");
            };
            if !cfg.adapters.is_empty() {
                anyhow::ensure!(cfg.adapters.iter().any(|ad| ad.id == a), "[model] fallbacks: unknown adapter {a:?} in {f:?}");
            }
        }
        anyhow::ensure!(cfg.backup.keep_days >= 1, "[backup] keep_days must be at least 1: with 0 every backup is deleted as soon as it is made");
        if let Some(b) = &cfg.browser {
            for site in &b.allow {
                anyhow::ensure!(
                    browser_allow_ok(site),
                    "[browser] allow: {site:?} is not a name, address or range: want `name`, `name:port`, `1.2.3.4`, `[::1]:8080` or `10.0.0.0/8`"
                );
            }
        }
        for c in &cfg.connectors {
            anyhow::ensure!(
                !c.name.contains("__") && c.name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'),
                "connector name {:?} must be letters, digits, '-' or '_' and must not contain '__'",
                c.name
            );
            for (t, class) in &c.tools {
                anyhow::ensure!(crate::tools::Risk::parse(&class.risk).is_some(), "tool {}.{t}: unknown risk class {}", c.name, class.risk);
                if let Some(r) = &class.reach {
                    anyhow::ensure!(
                        crate::tools::reach_fits(r, &class.risk),
                        "tool {}.{t}: reach {r:?} does not fit a {} tool; the only reach is \"self\", on a write that stays in the person's own account",
                        c.name,
                        class.risk
                    );
                }
            }
            for (t, d) in &c.destinations {
                let risk = |t: &str| c.tools.get(t).map(|class| class.risk.clone());
                anyhow::ensure!(risk(t).is_some(), "connector {}: destinations for {t}, which is not one of its tools", c.name);
                if let Some(why) = d.fault(t, risk) {
                    anyhow::bail!("connector {}: {why}", c.name);
                }
            }
            for (t, rb) in &c.read_back {
                let risk = |t: &str| c.tools.get(t).map(|class| class.risk.clone());
                anyhow::ensure!(risk(t).is_some(), "connector {}: read_back for {t}, which is not one of its tools", c.name);
                if let Some(why) = rb.fault(t, risk) {
                    anyhow::bail!("connector {}: {why}", c.name);
                }
            }
            if let Some(s) = &c.sent {
                let risk = |t: &str| c.tools.get(t).map(|class| class.risk.clone());
                if let Some(why) = s.fault(risk) {
                    anyhow::bail!("connector {}: {why}", c.name);
                }
            }
            anyhow::ensure!(crate::tools::Label::parse(&c.label).is_some(), "connector {}: unknown label {}", c.name, c.label);
            anyhow::ensure!(crate::tools::Audience::parse(&c.audience).is_some(), "connector {}: unknown audience {}", c.name, c.audience);
            let kind = crate::transport::Kind::parse(&c.transport).ok_or_else(|| anyhow::anyhow!("connector {}: unknown transport {}", c.name, c.transport))?;
            if kind == crate::transport::Kind::Brokered {
                for r in &cfg.brokered.roots {
                    std::fs::read(r).map_err(|e| anyhow::anyhow!("[brokered] roots: {}: {e}", r.display()))?;
                }
                crate::brokered::digest(&c.url).map_err(|e| anyhow::anyhow!("connector {}: {e}", c.name))?;
                anyhow::ensure!(!c.hosts.is_empty(), "connector {}: a brokered connector names the provider hosts it may reach", c.name);
                // Its calls carry no idempotency key to its server, so one
                // whose outcome is unknown cannot be sent again blindly: the
                // person is asked (`none`).
                anyhow::ensure!(c.idempotency != "header", "connector {}: a brokered connector's calls carry no idempotency key, so it cannot have idempotency = \"header\"; leave it out, and a call whose outcome is unknown is put to the person", c.name);
            }
        }
        Ok(cfg)
    }

    pub fn person(&self, id: &str) -> Option<&Person> {
        self.people.iter().find(|p| p.id == id)
    }

    pub fn person_by_token(&self, token: &str) -> Option<&Person> {
        use sha2::Digest;
        // Compare digests so the comparison time does not depend on the token.
        let want = sha2::Sha256::digest(token.as_bytes());
        self.people.iter().find(|p| p.token.as_ref().is_some_and(|t| sha2::Sha256::digest(t.as_bytes()) == want))
    }
}

/// Whether this `[browser] allow` entry parses as the sidecar reads it
/// (`browser/destinations.js`, `parseAllow`): a name or address with or
/// without `:port`, or a range. A port is 1-5 digits, as there. Checked at
/// load so a bad entry fails here naming itself, instead of stopping the
/// sidecar with only "the browser sidecar did not start" (#242).
/// As the sidecar's `net.isIPv6`, which accepts a zone: exactly one `%`,
/// and the zone is one or more of letters, digits, `-.:` (probed on Node
/// 24.21: `fe80::1%eth0` and even `fe80::1%eth0:8080` parse; `fe80::1%`,
/// a second `%`, nothing before it, and `_`, space, `@`, `+`, `~`, `[`
/// do not). Rust's parser takes no zone at all, so it is tested without.
fn is_ipv6(s: &str) -> bool {
    match s.split_once('%') {
        None => s.parse::<std::net::Ipv6Addr>().is_ok(),
        Some((ip, zone)) => !zone.is_empty() && zone.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b':')) && ip.parse::<std::net::Ipv6Addr>().is_ok(),
    }
}

fn browser_allow_ok(entry: &str) -> bool {
    let lower = entry.trim().to_lowercase();
    let s = lower.strip_suffix('.').unwrap_or(&lower);
    if s.contains('/') || is_ipv6(s) {
        return range_ok(s);
    }
    // `[host]` or `[host]:port`.
    if let Some(rest) = s.strip_prefix('[') {
        let Some((host, after)) = rest.split_once(']') else { return false };
        if host.is_empty() {
            return false;
        }
        let port = if after.is_empty() { None } else { after.strip_prefix(':') };
        if !after.is_empty() && port.is_none() {
            return false;
        }
        return port.is_none_or(port_ok) && ip_or_name_ok(host);
    }
    // `host` or `host:port`, with no colon in the host.
    let (host, port) = match s.rsplit_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (s, None),
    };
    if host.is_empty() || host.contains([':', '[', ']']) {
        return false;
    }
    port.is_none_or(port_ok) && ip_or_name_ok(host)
}

fn port_ok(p: &str) -> bool {
    (1..=5).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit())
}

/// An address must parse; anything else is a name, which the sidecar
/// resolves when it is used.
fn ip_or_name_ok(host: &str) -> bool {
    host.parse::<std::net::IpAddr>().is_ok() || !host.is_empty()
}

/// Whether this parses as the sidecar's `range`: an address, or an address
/// with a prefix length it has that many bits for.
fn range_ok(s: &str) -> bool {
    let (ip, len) = match s.split_once('/') {
        Some((ip, len)) => (ip, Some(len)),
        None => (s, None),
    };
    // As the sidecar's `bytes`: brackets and a zone come off first.
    let ip = ip.trim_start_matches('[').trim_end_matches(']');
    let ip = ip.split('%').next().unwrap_or("");
    let bits = match ip.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(_)) => 32,
        Ok(std::net::IpAddr::V6(_)) => 128,
        Err(_) => return false,
    };
    match len {
        None => true,
        Some(l) => l.parse::<u32>().is_ok_and(|n| n <= bits),
    }
}

/// The server's own time zone by its IANA name, as `chrono::Local` reads
/// the clock: `TZ` when it names a zone, else the zone `/etc/localtime`
/// links to, else `/etc/timezone`, else UTC. Read once, with the config.
pub(crate) fn server_zone_name() -> String {
    let named = |z: &str| z.trim().trim_start_matches(':').parse::<chrono_tz::Tz>().ok().map(|t| t.name().to_string());
    std::env::var("TZ")
        .ok()
        .and_then(|z| named(&z))
        .or_else(|| std::fs::read_link("/etc/localtime").ok().and_then(|p| p.to_string_lossy().split_once("zoneinfo/").and_then(|(_, z)| named(z))))
        .or_else(|| std::fs::read_to_string("/etc/timezone").ok().and_then(|z| named(&z)))
        .unwrap_or_else(|| "UTC".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::legal::TEST_CONFIG as LEGAL;

    fn load(connector: &str) -> anyhow::Result<Config> {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("magnus.toml");
        let text = format!("[appliance]\nstate_dir = \"/s\"\nlisten = \"127.0.0.1:0\"\n[database]\nurl = \"postgres://x\"\n[model]\nname = \"m\"\nadapter = \"a\"\nmax_tokens = 1\n{LEGAL}[[connectors]]\nname = \"mail\"\naudience = \"public\"\nlabel = \"private+untrusted\"\n{connector}");
        std::fs::write(&p, text).unwrap();
        Config::load(&p)
    }

    /// A server configured before the agreements still starts on an update:
    /// with no `[legal]` at all the agreements are off, and a partial one is
    /// still refused, naming what is missing.
    #[test]
    fn a_server_with_no_legal_section_still_starts_and_a_partial_one_does_not() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("magnus.toml");
        let base = "[appliance]\nstate_dir = \"/s\"\nlisten = \"127.0.0.1:0\"\n[database]\nurl = \"postgres://x\"\n[model]\nname = \"m\"\nadapter = \"a\"\nmax_tokens = 1\n";
        std::fs::write(&p, base).unwrap();
        assert!(!Config::load(&p).unwrap().legal.is_set());
        std::fs::write(&p, format!("{base}[legal]\ncontact = \"a@b.c\"\n")).unwrap();
        let e = Config::load(&p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains("operator"), "{e}");
    }

    /// The console is retired, and a server's config written for it still
    /// loads: its `[admin]` section, every field the console read, is ignored.
    #[test]
    fn the_retired_consoles_section_still_loads() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("magnus.toml");
        let admin = "[admin]\nhostname = \"admin.example\"\nlisten = [\"10.0.0.2:443\"]\nallow = [\"10.0.0.0/8\"]\ntls_cert = \"/c.pem\"\ntls_key = \"/k.pem\"\nidle_minutes = 30\ndatabase_url = \"postgres://magnus_root:x@h/m\"\ntrusted_via = \"tailscale\"\n";
        let text = format!(
            "[appliance]\nstate_dir = \"/s\"\nlisten = \"127.0.0.1:0\"\n[database]\nurl = \"postgres://x\"\n[model]\nname = \"m\"\nadapter = \"a\"\nmax_tokens = 1\n{LEGAL}{admin}"
        );
        std::fs::write(&p, text).unwrap();
        assert!(Config::load(&p).unwrap().admin.is_some());
    }

    /// A brokered connector's calls carry no idempotency key, so it is
    /// never taken to deduplicate: one that says it does is refused, and
    /// one that says nothing does not.
    #[test]
    fn a_brokered_connector_cannot_claim_to_deduplicate() {
        let brokered = format!("transport = \"brokered\"\nurl = \"sha256:{}\"\nhosts = [\"api.example\"]\n", "0a".repeat(32));
        let e = load(&format!("{brokered}idempotency = \"header\"\n")).unwrap_err();
        assert!(format!("{e:#}").contains("carry no idempotency key"), "{e:#}");
        assert_eq!(load(&brokered).unwrap().connectors[0].idempotency, "none");
        // A remote app may say it honors the header.
        assert!(load("url = \"https://a.example/mcp\"\nidempotency = \"header\"\n").is_ok());
    }

    fn load_backup(backup: &str) -> anyhow::Result<Config> {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("magnus.toml");
        let text = format!("[appliance]\nstate_dir = \"/s\"\nlisten = \"127.0.0.1:0\"\n[database]\nurl = \"postgres://x\"\n[model]\nname = \"m\"\nadapter = \"a\"\nmax_tokens = 1\n{LEGAL}[backup]\n{backup}");
        std::fs::write(&p, text).unwrap();
        Config::load(&p)
    }

    /// The schedule's waits come from `[backup]` (#289): a set value lands,
    /// an absent one keeps the old default.
    #[test]
    fn backup_waits_come_from_config() {
        let unset = load_backup("").unwrap().backup;
        assert_eq!(unset.fallback_wait_secs, 60);
        assert_eq!(unset.retry_wait_secs, 3600);
        let set = load_backup("fallback_wait_secs = 7\nretry_wait_secs = 120\n").unwrap().backup;
        assert_eq!(set.fallback_wait_secs, 7);
        assert_eq!(set.retry_wait_secs, 120);
    }

    /// The off-host destination comes from `[backup]` only (#297): none
    /// configured, no off-host copy.
    #[test]
    fn backup_copy_comes_from_config() {
        let unset = load_backup("").unwrap().backup;
        assert_eq!((unset.copy_to, unset.copy_timeout_secs), (None, 300));
        let set = load_backup("copy_to = \"strix-beta:/backups\"\ncopy_timeout_secs = 60\n").unwrap().backup;
        assert_eq!((set.copy_to.as_deref(), set.copy_timeout_secs), (Some("strix-beta:/backups"), 60));
    }

    fn load_browser(allow: &str) -> anyhow::Result<Config> {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("magnus.toml");
        let text = format!("[appliance]\nstate_dir = \"/s\"\nlisten = \"127.0.0.1:0\"\n[database]\nurl = \"postgres://x\"\n[model]\nname = \"m\"\nadapter = \"a\"\nmax_tokens = 1\n{LEGAL}[browser]\ncommand = [\"node\", \"sidecar.js\"]\nchromium = \"/c\"\nallow = [{allow}]");
        std::fs::write(&p, text).unwrap();
        Config::load(&p)
    }

    /// A bad `[browser] allow` entry fails at load naming itself and what
    /// was expected, instead of stopping the sidecar with only "the browser
    /// sidecar did not start". What parses mirrors the sidecar's
    /// `parseAllow` (`browser/destinations.js`).
    #[test]
    fn a_bad_browser_allow_entry_fails_at_load_naming_itself() {
        for good in [
            "\"printer\"",
            "\"printer.lan\"",
            "\"printer.lan:631\"",
            "\"intranet.\"",
            "\"EXAMPLE.com:443\"",
            "\"10.1.2.3\"",
            "\"10.1.2.3:8080\"",
            "\"[::1]:8080\"",
            "\"::1\"",
            "\"10.1.0.0/16\"",
            "\"fc00::/7\"",
            "\"10.1.0.0/8\"",
            "\"fe80::1%eth0\"",
            "\"FE80::1%ETH0\"",
            "\"fe80::1%eth0:8080\"",
        ] {
            assert!(load_browser(good).is_ok(), "{good}");
        }
        for bad in [
            "\"\"",
            "\"10.0.0.0/33\"",
            "\"10.0.0.0/8/8\"",
            "\"10.0.0.0/-1\"",
            "\"[::1\"",
            "\"[::1]x\"",
            "\"printer.lan:\"",
            "\"printer.lan:123456\"",
            "\"a:b:c\"",
            "\":8080\"",
            "\"fe80::1%\"",
            "\"fe80::1%a%b\"",
            "\"fe80::1%eth_0\"",
            "\"fe80::1%eth 0\"",
        ] {
            let e = load_browser(bad).unwrap_err();
            assert!(format!("{e:#}").contains("[browser] allow") && format!("{e:#}").contains(bad.trim_matches('"')), "{bad}: {e:#}");
        }
    }

    /// A tool's reach is `self` on a write, or nothing: a read reaches no
    /// one to begin with, money asks every time, and any other word is
    /// unknown (approvals spec, The approval model).
    #[test]
    fn a_tool_reaches_self_only_as_a_write() {
        let tool = |risk: &str, reach: &str| load(&format!("url = \"https://a.example/mcp\"\n[connectors.tools.draft]\nrisk = \"{risk}\"\nreach = \"{reach}\"\n"));
        assert_eq!(tool("reversible_write", "self").unwrap().connectors[0].tools["draft"].reach.as_deref(), Some("self"));
        assert!(tool("irreversible", "self").is_ok());
        for (risk, reach) in [("financial", "self"), ("read", "self"), ("prohibited", "self"), ("reversible_write", "everyone")] {
            let e = tool(risk, reach).unwrap_err();
            assert!(format!("{e:#}").contains("does not fit"), "{risk} {reach}: {e:#}");
        }
    }
}
