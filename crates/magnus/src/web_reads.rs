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
use crate::tools::Risk;
use anyhow::Result;

/// The reads whose results are where an address may be found: the public
/// web, never a page read through the person's own sign-in.
const SOURCES: [&str; 5] = ["web_search", "web_fetch", "browser_navigate", "browser_snapshot", "browser_back"];

/// Whether a web or browser read can carry out nothing but what it was
/// given. `known` says whether an address appeared in a source.
pub(crate) fn sends_nothing_new(a: &Action, f: &Facts, known: impl Fn(&str) -> bool) -> bool {
    if f.connector.is_some() || f.risk != Risk::Read || !matches!(f.app.as_str(), "web" | "browser") {
        return false;
    }
    match a.tool.as_str() {
        "browser_snapshot" | "browser_screenshot" | "browser_back" => true,
        "web_fetch" | "browser_navigate" => a.input["url"].as_str().is_some_and(|u| known(u.trim())),
        _ => false,
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
    let last = r.intents.values().filter(|i| i.tool == "browser_navigate" && i.result.as_ref().is_some_and(|res| !res.is_error)).max_by_key(|i| i.at)?;
    url::Url::parse(last.input["url"].as_str()?).ok()?.host_str().map(str::to_lowercase)
}

/// What the web returned to this run and the runs above it.
fn returned(s: &State, run: &str) -> Vec<String> {
    let tasks = s.lineage(run);
    let runs = tasks.iter().filter_map(|t| s.tasks.get(t)).filter_map(|t| s.runs.get(&t.run_id));
    runs.flat_map(|r| r.intents.values())
        .filter(|i| SOURCES.contains(&i.tool.as_str()))
        .filter_map(|i| i.result.as_ref().filter(|res| !res.is_error).map(|res| res.text.clone()))
        .collect()
}

impl Kernel {
    /// Whether this web read's address came from the person or the web.
    pub(crate) async fn plain_web_read(&self, a: &Action, f: &Facts, run: &Run) -> Result<bool> {
        let sources = self.web_sources(run).await?;
        Ok(sends_nothing_new(a, f, |u| appears_in(u, &sources)))
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
}
