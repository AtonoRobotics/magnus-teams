//! Web reads that carry nothing out (decisions/2026-11-12-web-reads-by-provenance.md).
//!
//! After a run has read private data and untrusted content, a web address
//! can carry that data out (K34). What carries it is an address the model
//! wrote. So, as Claude's web fetch and ChatGPT's link check do, the
//! address's origin decides, not an ask per page:
//! - a search query can transmit private data and keeps the boundary check;
//! - reading the page already open (snapshot, screenshot, back) sends nothing;
//! - opening an address that appears, exactly, in what the person wrote or
//!   in what a search or a page returned carries nothing the model added.
//!
//! Only an address the model made up itself still asks. That ask names the
//! site, and the person's Allow covers the site from then on (`asks::remembered`).
use crate::boundary::{Action, Facts};
use crate::kernel::Kernel;
use crate::state::{Run, State};
use crate::tool_registry::{navigates, web_read, WebRead};
use crate::tools::Risk;
use anyhow::Result;

/// Whether this read's results are where an address may be found: the
/// public web, never a page read through the person's own sign-in.
fn is_source(tool: &str) -> bool {
    matches!(web_read(tool), WebRead::Query | WebRead::OpensUrl | WebRead::OpenPage)
}

/// Whether a web or browser read can carry out nothing but what it was
/// given. `known` says whether an address appeared in a source.
pub(crate) fn sends_nothing_new(a: &Action, f: &Facts, known: impl Fn(&str) -> bool) -> bool {
    if f.connector.is_some() || f.risk != Risk::Read || !matches!(f.app.as_str(), "web" | "browser") {
        return false;
    }
    match web_read(&a.tool) {
        WebRead::OpenPage | WebRead::Picture => true,
        WebRead::OpensUrl => a.input["url"].as_str().is_some_and(|u| known(u.trim())),
        WebRead::Query | WebRead::Research | WebRead::No => false,
    }
}

/// Whether `url` appears, exactly, in one of `texts`. A trailing slash is
/// not data, so "https://a.example/" matches "https://a.example".
pub(crate) fn appears_in(url: &str, texts: &[String]) -> bool {
    let url = url.strip_suffix('/').unwrap_or(url);
    url::Url::parse(url).is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some())
        && texts.iter().any(|t| {
            t.split_whitespace().any(|token| {
                let token = if let Some((_, link)) = token.strip_prefix('[').and_then(|t| t.split_once("](")) {
                    link.strip_suffix(')').unwrap_or(token)
                } else {
                    [("<", ">"), ("(", ")"), ("[", "]"), ("\"", "\""), ("'", "'"), ("`", "`")]
                        .iter()
                        .find_map(|(left, right)| token.strip_prefix(left).and_then(|s| s.strip_suffix(right)))
                        .unwrap_or(token)
                };
                token.strip_suffix('/').unwrap_or(token) == url
            })
        })
}

/// The host of the page a browser action acts on: the address of the run's
/// last successful navigation.
pub(crate) fn page_host(s: &State, run: &str) -> Option<String> {
    let r = s.runs.get(run)?;
    let last = r.intents.values().filter(|i| navigates(&i.tool) && i.result.as_ref().is_some_and(|res| !res.is_error)).max_by_key(|i| i.at)?;
    url::Url::parse(last.input["url"].as_str()?).ok()?.host_str().map(str::to_lowercase)
}

/// What the web returned to this run and the runs above it.
fn returned(s: &State, run: &str) -> Vec<String> {
    let tasks = s.lineage(run);
    let runs = tasks.iter().filter_map(|t| s.tasks.get(t)).filter_map(|t| s.runs.get(&t.run_id));
    runs.flat_map(|r| r.intents.values()).filter(|i| is_source(&i.tool)).filter_map(|i| i.result.as_ref().filter(|res| !res.is_error).map(|res| res.text.clone())).collect()
}

/// Extract literal `owner/repo` pairs from the requester's original prompt text.
pub(crate) fn user_named_github_repos(prompt: &str) -> Vec<(String, String)> {
    let (_, dests) = crate::envelope::resolve::extract_literal_destinations(prompt);
    dests
        .into_iter()
        .filter_map(|d| match d {
            crate::envelope::Destination::GitHubRepo { owner, repo } => Some((owner, repo)),
            _ => None,
        })
        .collect()
}

/// Returns `true` if `raw_url` is a normalized HTTPS read URL under one of
/// the public GitHub prefixes for a repo in `repos` (`github.com`,
/// `api.github.com/repos`, or `raw.githubusercontent.com`), rejecting any
/// encoded-slash (`%2f`/`%5c`) or `..` traversal attempts (W1.7, N9).
pub(crate) fn is_allowed_github_repo_url(raw_url: &str, repos: &[(String, String)]) -> bool {
    if repos.is_empty() {
        return false;
    }
    let lower_raw = raw_url.trim().to_ascii_lowercase();
    if lower_raw.contains("%2f") || lower_raw.contains("%5c") || lower_raw.contains("..") {
        return false;
    }
    let Ok(parsed) = url::Url::parse(raw_url.trim()) else { return false };
    if parsed.scheme() != "https" || !parsed.username().is_empty() || parsed.password().is_some() {
        return false;
    }
    let Some(host) = parsed.host_str().map(str::to_ascii_lowercase) else { return false };
    let segs: Vec<&str> = parsed.path().split('/').filter(|s| !s.is_empty()).collect();
    let (owner, repo) = match host.as_str() {
        "github.com" | "raw.githubusercontent.com" => match (segs.first(), segs.get(1)) {
            (Some(o), Some(r)) => (*o, *r),
            _ => return false,
        },
        "api.github.com" => match (segs.first(), segs.get(1), segs.get(2)) {
            (Some(&"repos"), Some(o), Some(r)) => (*o, *r),
            _ => return false,
        },
        _ => return false,
    };
    let owner_l = owner.to_ascii_lowercase();
    let repo_l = repo.strip_suffix(".git").unwrap_or(repo).to_ascii_lowercase();
    repos.iter().any(|(o, r)| *o == owner_l && *r == repo_l)
}

/// Returns `true` if `raw_url` is a clean public HTTPS URL (domain host, no
/// userinfo, no query string, no traversal/encoded slashes, and passes
/// deterministic secret/PII scrubbing) that an `OpenWeb` worker—which holds
/// zero private data and whose query already passed `query_gate`—may open
/// directly without needing the user or search to provide the exact URL first.
pub(crate) fn is_clean_open_web_url(raw_url: &str) -> bool {
    let trimmed = raw_url.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("%2f") || lower.contains("%5c") || lower.contains("..") {
        return false;
    }
    let Ok(parsed) = url::Url::parse(trimmed) else { return false };
    if parsed.scheme() != "https" || !parsed.username().is_empty() || parsed.password().is_some() || parsed.query().is_some() {
        return false;
    }
    if !matches!(parsed.host(), Some(url::Host::Domain(d)) if d.contains('.')) {
        return false;
    }
    crate::compartment::query_gate::scrub_check(trimmed).is_none()
}

impl Kernel {
    /// Whether this web read's address came from the person or the web.
    pub(crate) async fn plain_web_read(&self, a: &Action, f: &Facts, run: &Run) -> Result<bool> {
        let sources = self.web_sources(run).await?;
        Ok(sends_nothing_new(a, f, |u| appears_in(u, &sources)))
    }

    /// Provenance & clean-URL check for an `OpenWeb` worker spawned from a
    /// tainted `Private` run: allows URLs from this worker's own search
    /// results, pages it loaded, the user's original prompt, user-named
    /// public GitHub repos, or clean public HTTPS URLs (no query string, no
    /// userinfo, passes secret/PII scrub).
    pub(crate) async fn open_web_link_allowed(&self, a: &Action, f: &Facts, run: &Run) -> Result<bool> {
        let requester = self.requester_run(run);
        let prompt = if requester.task.is_empty() { String::new() } else { self.asked(&requester).await? };
        let prompt_repos = user_named_github_repos(&prompt);
        let mut texts = if prompt.is_empty() { vec![] } else { vec![prompt] };
        texts.extend(self.read(|s| {
            s.runs
                .get(&run.id)
                .into_iter()
                .flat_map(|r| r.intents.values())
                .filter(|i| is_source(&i.tool))
                .filter_map(|i| i.result.as_ref().filter(|res| !res.is_error).map(|res| res.text.clone()))
                .collect::<Vec<_>>()
        }));
        Ok(sends_nothing_new(a, f, |u| appears_in(u, &texts) || is_allowed_github_repo_url(u, &prompt_repos) || is_clean_open_web_url(u)))
    }

    /// Where an action goes (`asks::destinations`), and for a browser
    /// action with no address of its own, the site of the page it acts on:
    /// so its ask names the site and the Allow is remembered for it.
    pub(crate) fn destinations_of(&self, a: &Action, f: &Facts, shown: Option<&crate::boundary::Shown>) -> Vec<String> {
        let d = crate::asks::destinations(a, f, shown);
        if !d.is_empty() || f.app != "browser" || f.connector.is_some() {
            return d;
        }
        self.read(|s| page_host(s, &a.run)).into_iter().collect()
    }

    /// Where an address may come from without carrying anything the model
    /// added: the person's own words and what the web returned to the work.
    pub(crate) async fn web_sources(&self, run: &Run) -> Result<Vec<String>> {
        let requester = self.requester_run(run);
        let mut texts = if requester.task.is_empty() { vec![] } else { vec![self.asked(&requester).await?] };
        texts.extend(self.read(|s| returned(s, &run.id)));
        Ok(texts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boundary::Issuer;
    use crate::tools::{Audience, Label};
    use serde_json::json;

    fn facts(app: &str, risk: Risk) -> Facts {
        Facts { name: String::new(), app: app.into(), risk, label: Label::UNTRUSTED, audience: Audience::Public, connector: None, remote: String::new(), reaches_self: false }
    }

    fn action(tool: &str, input: serde_json::Value) -> Action {
        Action { run: "r".into(), call_id: "c".into(), step_id: "s".into(), tool: tool.into(), input, issuer: Issuer::Model }
    }

    #[test]
    fn the_open_page_carries_nothing_out() {
        for t in ["browser_snapshot", "browser_screenshot", "browser_back"] {
            assert!(sends_nothing_new(&action(t, json!({})), &facts("browser", Risk::Read), |_| false), "{t} asked");
        }
    }

    #[test]
    fn only_an_address_the_model_made_up_can_carry_data_out() {
        let page = vec!["Results: https://news.example/story-1 and https://shop.example/".to_string()];
        let known = |u: &str| appears_in(u, &page);
        let fetch = |u: &str| sends_nothing_new(&action("web_fetch", json!({ "url": u })), &facts("web", Risk::Read), known);
        assert!(fetch("https://news.example/story-1"), "a link the search returned asked");
        assert!(fetch("https://shop.example"), "a trailing slash made a known link unknown");
        assert!(!fetch("https://news.example/story-1?d=revenue-41.7M"), "an address with data added went without an ask");
        assert!(!fetch("https://collect.example/revenue"), "a made-up address went without an ask");
        assert!(!fetch("https://"), "an empty address matched everything");
    }

    #[test]
    fn searches_do_not_exempt_private_queries() {
        assert!(!sends_nothing_new(&action("web_search", json!({ "query": "private salary" })), &facts("web", Risk::Read), |_| false));
        assert!(!sends_nothing_new(&action("browser_navigate", json!({ "url": "https://www.google.com/search?q=private+salary" })), &facts("browser", Risk::Read), |_| false));
    }

    #[test]
    fn provenance_requires_the_complete_url() {
        let texts = vec!["[link](https://example.com/collect?value=123456)".into()];
        assert!(appears_in("https://example.com/collect?value=123456", &texts));
        assert!(!appears_in("https://example.com/collect?value=123", &texts));
        assert!(!appears_in("https://example.com", &texts));
        assert!(!appears_in("https://example.com/123", &["https://other.example/](https://example.com/123)".into()]));
        for suffix in ["'secret", "(secret)", "[secret]"] {
            assert!(!appears_in("https://example.com/123", &[format!("https://example.com/123{suffix}")]));
        }
    }

    #[test]
    fn a_click_or_a_signed_in_page_is_not_a_plain_read() {
        assert!(!sends_nothing_new(&action("browser_click", json!({})), &facts("browser", Risk::ReversibleWrite), |_| true));
        let mut f = facts("browser", Risk::Read);
        f.connector = Some(serde_json::from_value(json!({ "name": "b", "transport": "http", "url": "http://x", "audience": "public", "label": "private" })).unwrap());
        assert!(!sends_nothing_new(&action("browser_snapshot", json!({})), &f, |_| true));
    }

    #[test]
    fn user_named_github_repos_allow_only_normalized_prefixes() {
        let repos = user_named_github_repos("Read-only review AtonoRobotics/magnus-teams main CI and tests.");
        assert_eq!(repos, vec![("atonorobotics".to_string(), "magnus-teams".to_string())]);
        assert!(is_allowed_github_repo_url("https://api.github.com/repos/AtonoRobotics/magnus-teams/contents/.github/workflows", &repos));
        assert!(is_allowed_github_repo_url("https://github.com/AtonoRobotics/magnus-teams/blob/main/ci/fast.sh", &repos));
        assert!(is_allowed_github_repo_url("https://raw.githubusercontent.com/AtonoRobotics/magnus-teams/main/ci/fast.sh", &repos));

        // Different repo, traversal, encoded slash, or non-https must be rejected.
        assert!(!is_allowed_github_repo_url("https://api.github.com/repos/AtonoRobotics/other-repo/contents", &repos));
        assert!(!is_allowed_github_repo_url("https://api.github.com/repos/AtonoRobotics/magnus-teams/../../other/repo", &repos));
        assert!(!is_allowed_github_repo_url("https://api.github.com/repos/AtonoRobotics/magnus-teams%2F..%2Fother", &repos));
        assert!(!is_allowed_github_repo_url("http://api.github.com/repos/AtonoRobotics/magnus-teams", &repos));
        assert!(!is_allowed_github_repo_url("https://evil.test/AtonoRobotics/magnus-teams", &repos));
    }

    #[test]
    fn clean_open_web_urls_allow_public_https_and_reject_queries_and_secrets() {
        assert!(is_clean_open_web_url("https://docs.rs"));
        assert!(is_clean_open_web_url("https://stripe.com/pricing"));
        assert!(is_clean_open_web_url("https://github.com/rust-lang/rust"));

        // Query parameters, HTTP, IP literals, userinfo, traversal, and secrets are rejected.
        assert!(!is_clean_open_web_url("https://evil.test/collect?d=revenue-41.7M"));
        assert!(!is_clean_open_web_url("http://docs.rs/tokio"));
        assert!(!is_clean_open_web_url("https://127.0.0.1/admin"));
        assert!(!is_clean_open_web_url("https://user:pass@stripe.com/pricing"));
        assert!(!is_clean_open_web_url("https://stripe.com/a/../b"));
        assert!(!is_clean_open_web_url("https://evil.test/exfil/sk-1234567890abcdef"));
        assert!(!is_clean_open_web_url("https://evil.test/exfil/a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7"));
    }
}
