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

/// The hosts a page's own markup loads from.
pub fn resources_of(html: &str) -> BTreeSet<String> {
    let mut hosts = BTreeSet::new();
    for attribute in ["src=", "href="] {
        let mut rest = html;
        while let Some(at) = rest.find(attribute) {
            rest = &rest[at + attribute.len()..];
            let quoted = rest.strip_prefix('"').or_else(|| rest.strip_prefix('\''));
            let Some(after_quote) = quoted else { continue };
            let end = after_quote.find(['"', '\'']).unwrap_or(after_quote.len());
            if let Some(host) = url::Url::parse(&after_quote[..end]).ok().and_then(|u| u.host_str().map(String::from)) {
                hosts.insert(host);
            }
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
        learned.insert("p1".into(), resources_of(r#"<img src="https://cdn.test/a.png"><a href='https://link.test/'>x</a>"#));
        assert!(classify(&[], &c, &learned, &|_| false, "news.test", 443, "GET").is_ok());
        assert!(classify(&[], &c, &learned, &|_| false, "cdn.test", 443, "GET").is_ok());
        assert!(classify(&[], &c, &learned, &|_| false, "cdn.test", 443, "POST").is_err(), "a page's resource took a write");
        c.pages.clear();
        assert!(classify(&[], &c, &learned, &|_| false, "cdn.test", 443, "GET").is_err(), "a closed page's resource went through");
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
