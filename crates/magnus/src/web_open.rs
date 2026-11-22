//! Which web addresses a run opens on its own after it has read untrusted
//! content and private data, with no card and no model's judgement
//! (decisions/2026-11-22-browsing-without-asks.md).
//!
//! An address carries data out only to someone who sees the request. So,
//! besides an address the person wrote or the web returned (`web_reads`),
//! a run opens on its own:
//! - any address on a site the person named in their own words;
//! - the bare home page of one of the most-visited sites, which carries
//!   nothing and whose visits an attacker cannot see.
//!
//! Anything else is refused without a card, and the agent is told to open the
//! site's home page or search, then follow links. A deep path on a popular
//! site is not opened on its own: some such sites show their page owners
//! which paths were visited.

use crate::kernel::Kernel;
use crate::state::Run;
use anyhow::Result;
use std::collections::HashSet;
use std::sync::OnceLock;

/// Origins of rank 10,000 or better in the Chrome UX Report, scheme removed.
const POPULAR_SITES: &str = include_str!("../data/popular_sites.txt");

/// What the agent is told when an address it wrote itself is refused.
pub(crate) const WRITTEN_ADDRESS_REFUSED: &str = "this conversation holds private data and untrusted content, so an address you wrote yourself opens only when it is a site's bare home page or on a site the person named. Open the site's home page (https://example.com) or search for the page, then follow its links";

fn popular() -> &'static HashSet<&'static str> {
    static SITES: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SITES.get_or_init(|| POPULAR_SITES.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).collect())
}

/// Whether `host` is one of the most-visited sites, with or without `www.`.
fn is_popular_host(host: &str) -> bool {
    let h = host.trim_end_matches('.').to_ascii_lowercase();
    let set = popular();
    set.contains(h.as_str()) || set.contains(format!("www.{h}").as_str()) || h.strip_prefix("www.").is_some_and(|bare| set.contains(bare))
}

/// An https home page with nothing after the host, on a most-visited site.
pub(crate) fn is_bare_popular_origin(raw: &str) -> bool {
    let Ok(u) = url::Url::parse(raw.trim()) else { return false };
    u.scheme() == "https"
        && u.username().is_empty()
        && u.password().is_none()
        && u.port().is_none()
        && matches!(u.path(), "" | "/")
        && u.query().is_none()
        && u.fragment().is_none()
        && matches!(u.host(), Some(url::Host::Domain(d)) if is_popular_host(d))
}

/// The host a word names, with or without a scheme: `stripe.com`,
/// `https://docs.stripe.com/api`. Never an email address.
fn host_of_word(word: &str) -> Option<String> {
    let w = word.trim_matches(|c: char| matches!(c, ',' | ';' | ':' | '(' | ')' | '[' | ']' | '<' | '>' | '"' | '\'' | '.' | '`' | '!' | '?'));
    if w.is_empty() || w.contains('@') {
        return None;
    }
    let candidate = if w.starts_with("http://") || w.starts_with("https://") { w.to_string() } else { format!("https://{w}") };
    let u = url::Url::parse(&candidate).ok()?;
    let Some(url::Host::Domain(d)) = u.host() else { return None };
    let d = d.to_ascii_lowercase();
    let tld = d.rsplit('.').next()?;
    (d.contains('.') && tld.len() >= 2 && tld.bytes().all(|b| b.is_ascii_alphabetic())).then_some(d)
}

/// The hosts the person named in `words`.
pub(crate) fn named_hosts(words: &str) -> Vec<String> {
    let mut hosts: Vec<String> = words.split_whitespace().filter_map(host_of_word).collect();
    hosts.sort();
    hosts.dedup();
    hosts
}

/// Whether `raw` is a web address on one of `hosts` or under it.
pub(crate) fn on_named_host(raw: &str, hosts: &[String]) -> bool {
    let Ok(u) = url::Url::parse(raw.trim()) else { return false };
    if !matches!(u.scheme(), "http" | "https") || !u.username().is_empty() || u.password().is_some() {
        return false;
    }
    let Some(url::Host::Domain(d)) = u.host() else { return false };
    let d = d.to_ascii_lowercase();
    let d = d.strip_prefix("www.").unwrap_or(&d);
    hosts.iter().map(|h| h.strip_prefix("www.").unwrap_or(h)).any(|h| d == h || d.ends_with(&format!(".{h}")))
}

impl Kernel {
    /// Whether a run that holds untrusted content and private data opens
    /// `raw`, an address the model wrote, on its own.
    pub(crate) async fn opens_on_its_own(&self, run: &Run, raw: &str) -> Result<bool> {
        if is_bare_popular_origin(raw) {
            return Ok(true);
        }
        let requester = self.requester_run(run);
        if requester.task.is_empty() {
            return Ok(false);
        }
        let words = self.asked(&requester).await?;
        Ok(on_named_host(raw, &named_hosts(&words)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_popular_home_page_opens_and_nothing_after_it_does() {
        assert!(is_bare_popular_origin("https://github.com"));
        assert!(is_bare_popular_origin("https://github.com/"));
        assert!(is_bare_popular_origin("https://www.google.com"));
        // The CRM injection's collector, and data anywhere in an address.
        assert!(!is_bare_popular_origin("https://collector.example/collect?d=revenue-41.7M"));
        assert!(!is_bare_popular_origin("https://github.com/attacker/repo/revenue-41.7M"));
        assert!(!is_bare_popular_origin("https://github.com/?q=revenue"));
        assert!(!is_bare_popular_origin("https://github.com/#revenue"));
        assert!(!is_bare_popular_origin("https://revenue-41-7m.github.com"));
        assert!(!is_bare_popular_origin("https://user:pass@github.com"));
        assert!(!is_bare_popular_origin("https://github.com:8443"));
        assert!(!is_bare_popular_origin("http://github.com"));
        assert!(!is_bare_popular_origin("https://not-a-popular-site.example"));
    }

    #[test]
    fn the_list_is_the_most_visited_sites() {
        assert!(popular().len() > 9_000, "{}", popular().len());
        assert!(popular().iter().all(|h| !h.contains('/') && !h.contains(':')));
    }

    #[test]
    fn a_site_the_person_named_opens_at_any_depth() {
        let hosts = named_hosts("Compare the plans on stripe.com and https://docs.example.org/spec, then email jane@company.com.");
        assert_eq!(hosts, vec!["docs.example.org".to_string(), "stripe.com".to_string()]);
        assert!(on_named_host("https://stripe.com/pricing", &hosts));
        assert!(on_named_host("https://www.stripe.com/pricing?plan=pro", &hosts));
        assert!(on_named_host("https://docs.stripe.com/api", &hosts));
        assert!(on_named_host("https://docs.example.org/other", &hosts));
        assert!(!on_named_host("https://company.com/", &hosts), "an email's domain is not a named site");
        assert!(!on_named_host("https://stripe.com.evil.example/x", &hosts));
        assert!(!on_named_host("https://evilstripe.com/x", &hosts));
        assert!(!on_named_host("https://user@stripe.com/x", &hosts));
        assert!(named_hosts("Read my inbox and the news.").is_empty());
    }
}
