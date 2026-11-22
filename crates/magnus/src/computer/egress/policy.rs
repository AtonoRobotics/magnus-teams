//! Which destinations a computer may reach. A guest with root says nothing
//! here: every input is the request itself and what the log holds.

use crate::computer::Role;
use crate::computer::{Computer, Status};
use std::collections::{BTreeSet, HashMap};

const READ_METHODS: [&str; 2] = ["GET", "HEAD"];
const WEB_PORTS: [u16; 2] = [80, 443];

fn matches(pattern: &str, host: &str) -> bool {
    match pattern.strip_prefix("*.") {
        Some(suffix) => host.len() > suffix.len() + 1 && host.ends_with(suffix) && host.as_bytes()[host.len() - suffix.len() - 1] == b'.',
        None => pattern.eq_ignore_ascii_case(host),
    }
}

/// Hosts each open page was seen to use, learned from the page itself.
pub type Learned = HashMap<String, BTreeSet<String>>;

/// `Ok` names the rule that allows the request; `Err` says why not.
/// `live` says whether a task is still running: a source granted to a
/// task is reachable only while it is.
pub fn classify(registries: &[String], c: &Computer, learned: &Learned, live: &dyn Fn(&str) -> bool, host: &str, port: u16, method: &str) -> Result<&'static str, String> {
    if c.status != Status::Running {
        return Err("the computer is not running".to_string());
    }
    if !WEB_PORTS.contains(&port) {
        return Err(format!("port {port} is not a web port"));
    }
    if matches!(c.role, Role::Throwaway { .. }) {
        return Ok("a throwaway browser");
    }
    let read = READ_METHODS.contains(&method);
    if c.apps.iter().flat_map(|a| &a.hosts).any(|h| matches(h, host)) {
        return Ok("declared by an app");
    }
    if c.destinations.iter().any(|h| matches(h, host)) {
        return Ok("added at the boundary");
    }
    if c.task_destinations.iter().any(|(h, task)| matches(h, host) && live(task)) {
        return Ok("granted to a running task");
    }
    if registries.iter().any(|h| matches(h, host)) {
        return if read { Ok("package registry") } else { Err(format!("a package registry is for downloads only, not {method}")) };
    }
    for page in &c.pages {
        if page.host.eq_ignore_ascii_case(host) || learned.get(&page.id).is_some_and(|hosts| hosts.iter().any(|h| h.eq_ignore_ascii_case(host))) {
            return if read { Ok("a page the agent opened") } else { Err(format!("a page the agent opened is only read, not {method}")) };
        }
    }
    Err(format!("{host} is not a declared destination"))
}

/// Maximum number of subresource hosts learned per open page.
pub const MAX_LEARNED_HOSTS_PER_PAGE: usize = 32;

/// Exact script/style URLs referenced by an open page's own HTML (1-level-deep
/// SPA host learning only; off-page URLs fetched by the guest never learn hosts).
pub type PageAssets = HashMap<String, BTreeSet<String>>;

const EMBEDDED_SRC_TAGS: [&str; 6] = ["script", "img", "source", "iframe", "video", "audio"];
const EMBEDDED_HREF_TAGS: [&str; 1] = ["link"];

/// Parse HTML tags and return `(hosts, script_or_style_urls)` referenced by
/// embedded resource elements (`<script src>`, `<link href>`, `<img src/srcset>`,
/// `<source>`, `<iframe src>`, `<video>`, `<audio>`). Navigation links
/// (`<a href>`) are deliberately excluded.
pub fn html_resources(html: &str, base_url: Option<&url::Url>) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut hosts = BTreeSet::new();
    let mut asset_urls = BTreeSet::new();
    let mut rest = html;
    while let Some(lt) = rest.find('<') {
        rest = &rest[lt + 1..];
        let Some(gt) = rest.find('>') else { break };
        let tag_body = &rest[..gt];
        rest = &rest[gt + 1..];
        let trimmed = tag_body.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('/') || trimmed.starts_with('!') || trimmed.starts_with('?') {
            continue;
        }
        let name_end = trimmed.find(|c: char| c.is_ascii_whitespace() || c == '/').unwrap_or(trimmed.len());
        let tag_name = trimmed[..name_end].to_ascii_lowercase();
        let attrs = &trimmed[name_end..];
        let is_src_tag = EMBEDDED_SRC_TAGS.contains(&tag_name.as_str());
        let is_href_tag = EMBEDDED_HREF_TAGS.contains(&tag_name.as_str());
        if !is_src_tag && !is_href_tag {
            continue;
        }
        let mut record_url = |raw: &str, is_asset: bool| {
            let parsed = url::Url::parse(raw).ok().or_else(|| base_url.and_then(|b| b.join(raw).ok()));
            let Some(u) = parsed else { return };
            if !matches!(u.scheme(), "http" | "https") {
                return;
            }
            if let Some(host) = u.host_str() {
                if hosts.len() < MAX_LEARNED_HOSTS_PER_PAGE {
                    hosts.insert(host.to_ascii_lowercase());
                }
            }
            if is_asset {
                asset_urls.insert(u.as_str().to_string());
            }
        };
        if is_src_tag {
            for val in attr_values(attrs, "src") {
                record_url(&val, tag_name == "script");
            }
            for val in attr_values(attrs, "srcset") {
                for entry in val.split(',') {
                    if let Some(u) = entry.split_whitespace().next() {
                        record_url(u, false);
                    }
                }
            }
        }
        if is_href_tag {
            for val in attr_values(attrs, "href") {
                record_url(&val, true);
            }
        }
    }
    (hosts, asset_urls)
}

fn attr_values(attrs: &str, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let lower = attrs.to_ascii_lowercase();
    let needle = format!("{name}=");
    let mut offset = 0;
    while let Some(pos) = lower[offset..].find(&needle) {
        let abs = offset + pos;
        let boundary_ok = abs == 0 || attrs.as_bytes()[abs - 1].is_ascii_whitespace();
        let val_start = abs + needle.len();
        offset = val_start;
        if !boundary_ok {
            continue;
        }
        let after = attrs[val_start..].trim_start();
        let Some(first) = after.chars().next() else { continue };
        let val = if first == '"' || first == '\'' {
            let inner = &after[1..];
            let end = inner.find(first).unwrap_or(inner.len());
            &inner[..end]
        } else {
            let end = after.find(|c: char| c.is_ascii_whitespace() || c == '>').unwrap_or(after.len());
            &after[..end]
        };
        if !val.is_empty() {
            out.push(val.to_string());
        }
    }
    out
}

/// Extract HTTP(S) host literals from a JS or CSS asset body that was
/// explicitly referenced by an open page's own HTML (1 level deep, capped).
pub fn hosts_from_asset_body(body: &str, remaining_cap: usize) -> BTreeSet<String> {
    let mut hosts = BTreeSet::new();
    if remaining_cap == 0 {
        return hosts;
    }
    for scheme in ["https://", "http://"] {
        let mut rest = body;
        while let Some(at) = rest.find(scheme) {
            rest = &rest[at..];
            let end = rest.find(|c: char| c.is_ascii_whitespace() || matches!(c, '"' | '\'' | '`' | ')' | ']' | '}' | '<' | '>' | ',' | ';')).unwrap_or(rest.len());
            let candidate = &rest[..end];
            if let Some(host) = url::Url::parse(candidate).ok().and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase())) {
                hosts.insert(host);
                if hosts.len() >= remaining_cap {
                    return hosts;
                }
            }
            rest = &rest[scheme.len()..];
        }
    }
    hosts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computer::{App, Change, Ledger, OpenPage, Owner};

    fn computer(ledger: &mut Ledger) -> Computer {
        ledger.apply(&Change::Created { id: "a".into(), owner: Owner::Person("p".into()) });
        ledger.apply(&Change::Started { id: "a".into() });
        ledger.get("a").unwrap().clone()
    }

    #[test]
    fn only_what_is_declared_gets_through() {
        let mut l = Ledger::default();
        let mut c = computer(&mut l);
        let reg = vec!["pypi.org".to_string(), "*.pythonhosted.org".to_string()];
        let none = Learned::new();
        assert!(classify(&reg, &c, &none, &|_| false, "evil.test", 443, "GET").is_err());
        assert!(classify(&reg, &c, &none, &|_| false, "pypi.org", 443, "GET").is_ok());
        assert!(classify(&reg, &c, &none, &|_| false, "files.pythonhosted.org", 443, "GET").is_ok());
        assert!(classify(&reg, &c, &none, &|_| false, "pypi.org", 443, "POST").is_err(), "a registry took an upload");
        assert!(classify(&reg, &c, &none, &|_| false, "notpypi.org", 443, "GET").is_err());
        assert!(classify(&reg, &c, &none, &|_| false, "xpythonhosted.org", 443, "GET").is_err());
        assert!(classify(&reg, &c, &none, &|_| false, "pypi.org", 22, "GET").is_err(), "a non-web port went through");
        c.apps.push(App { name: "n".into(), version: "1".into(), source: "s".into(), sha256: "h".into(), steps: vec![], hosts: vec!["api.app.test".into()] });
        assert!(classify(&reg, &c, &none, &|_| false, "api.app.test", 443, "POST").is_ok());
    }

    #[test]
    fn a_page_opens_itself_and_its_resources_and_only_while_open() {
        let mut l = Ledger::default();
        let mut c = computer(&mut l);
        c.pages.push(OpenPage { id: "p1".into(), host: "news.test".into() });
        let mut learned = Learned::new();
        let (hosts, assets) =
            html_resources(r#"<img src="https://cdn.test/a.png"><script src="https://cdn.jsdelivr.net/npm/app@1/main.js"></script><a href='https://link.test/'>x</a>"#, None);
        assert!(hosts.contains("cdn.test"));
        assert!(hosts.contains("cdn.jsdelivr.net"));
        assert!(!hosts.contains("link.test"), "<a href> must not be learned as a subresource");
        assert!(assets.contains("https://cdn.jsdelivr.net/npm/app@1/main.js"));
        // An off-page script on the same learned CDN is NOT in assets, so its
        // body must never teach further hosts (P2).
        assert!(!assets.contains("https://cdn.jsdelivr.net/npm/attacker-pkg/x.js"));
        learned.insert("p1".into(), hosts);
        assert!(classify(&[], &c, &learned, &|_| false, "news.test", 443, "GET").is_ok());
        assert!(classify(&[], &c, &learned, &|_| false, "cdn.test", 443, "GET").is_ok());
        assert!(classify(&[], &c, &learned, &|_| false, "link.test", 443, "GET").is_err());
        assert!(classify(&[], &c, &learned, &|_| false, "cdn.test", 443, "POST").is_err(), "a page's resource took a write");
        c.pages.clear();
        assert!(classify(&[], &c, &learned, &|_| false, "cdn.test", 443, "GET").is_err(), "a closed page's resource went through");
    }

    #[test]
    fn asset_body_learning_is_capped() {
        let mut js = String::new();
        for i in 0..50 {
            js.push_str(&format!("fetch('https://api{i}.example.test/v1');\n"));
        }
        let found = hosts_from_asset_body(&js, MAX_LEARNED_HOSTS_PER_PAGE);
        assert_eq!(found.len(), MAX_LEARNED_HOSTS_PER_PAGE);
    }

    /// A source granted to one task is reached while that task runs, and
    /// not once it ends or from a later task.
    #[test]
    fn a_source_granted_to_a_task_lasts_only_while_it_runs() {
        let mut l = Ledger::default();
        computer(&mut l);
        l.apply(&Change::DestinationGranted { id: "a".into(), host: "data.example".into(), task: "t1".into() });
        let c = l.get("a").unwrap().clone();
        let running = |t: &str| t == "t1";
        assert_eq!(classify(&[], &c, &Learned::new(), &running, "data.example", 443, "POST"), Ok("granted to a running task"));
        assert!(classify(&[], &c, &Learned::new(), &|t| t == "t2", "data.example", 443, "GET").is_err(), "another task used the source");
        assert!(classify(&[], &c, &Learned::new(), &|_| false, "data.example", 443, "GET").is_err(), "the source outlived its task");
        assert!(classify(&[], &c, &Learned::new(), &running, "other.example", 443, "GET").is_err());
    }

    #[test]
    fn a_computer_that_is_not_running_reaches_nothing() {
        let mut l = Ledger::default();
        let mut c = computer(&mut l);
        c.status = Status::Paused;
        assert!(classify(&["pypi.org".to_string()], &c, &Learned::new(), &|_| false, "pypi.org", 443, "GET").is_err());
    }
}
