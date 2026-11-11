//! Overview's "anything that needs an admin: an agent stopped" (spec rev
//! 141, Overview): every kill switch below the server's that is on (one
//! person's agents, one agent, one connector) and every paused agent is an
//! alert, naming who did it and when, fixed in Agents where each is released
//! or resumed. The server's own switch is Overview's first alert already.

use crate::halts::TEAM_AGENT;
use serde_json::{json, Value};

/// When, as an alert says it.
fn at(v: &Value) -> String {
    v.as_str().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|t| format!(" at {}", t.with_timezone(&chrono::Utc).format("%Y-%m-%d %H:%M UTC"))).unwrap_or_default()
}

/// An agent as an alert names it: the team agent, or a person's.
fn agent_name(agent: &str, name_of: &dyn Fn(&str) -> String) -> String {
    match agent.strip_prefix("person:") {
        Some(p) => format!("{}'s agent", name_of(p)),
        None if agent == TEAM_AGENT => "the team agent".into(),
        None => agent.to_string(),
    }
}

/// The alerts for `switches` (`Kernel::switches_all`'s answer): one for each
/// switch on below the server's and each paused agent. `name_of` gives a
/// person's name from their id.
pub fn stop_alerts(switches: &Value, name_of: &dyn Fn(&str) -> String) -> Vec<Value> {
    let mut out = vec![];
    for s in switches["on"].as_array().into_iter().flatten() {
        let who = s["by_name"].as_str().or(s["by"].as_str()).unwrap_or("An admin");
        let target = s["target"].as_str().unwrap_or_default();
        let what = match s["level"].as_str() {
            Some("person") => format!("{}'s agents", name_of(target)),
            Some("agent") => agent_name(target, name_of),
            Some("connector") => format!("the connector {target}"),
            _ => "some work".into(),
        };
        out.push(json!({ "what": format!("{who} stopped {what}{}.", at(&s["at"])), "fix": "agents", "level": s["level"], "target": target }));
    }
    for p in switches["paused"].as_array().into_iter().flatten() {
        let who = p["name"].as_str().or(p["by"].as_str()).unwrap_or("An admin");
        let agent = p["agent"].as_str().unwrap_or_default();
        out.push(json!({ "what": format!("{who} paused {}{}: nothing new starts for it.", agent_name(agent, name_of), at(&p["at"])), "fix": "agents", "level": "paused", "target": agent }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::stop_alerts;
    use serde_json::json;

    /// A person's, an agent's or a connector's switch, and a paused agent,
    /// each need an admin: Overview names who did it and when.
    #[test]
    fn every_stop_below_the_server_is_an_alert() {
        let sw = json!({
            "server": { "engaged": false, "pulled": null },
            "on": [
                { "level": "person", "target": "mo", "by": "admin:ana", "by_name": "Ana", "at": "2026-11-11T09:30:00Z" },
                { "level": "agent", "target": "team", "by": "admin:ana", "by_name": "Ana", "at": "2026-11-11T09:31:00Z" },
                { "level": "connector", "target": "gmail", "by": "admin:lee", "by_name": "Lee", "at": "2026-11-11T09:32:00Z" },
            ],
            "paused": [{ "agent": "person:mo", "by": "admin:ana", "name": "Ana", "at": "2026-11-11T09:33:00Z" }],
        });
        let name_of = |id: &str| if id == "mo" { "Mo".to_string() } else { id.to_string() };
        let alerts = stop_alerts(&sw, &name_of);
        let said: Vec<&str> = alerts.iter().filter_map(|a| a["what"].as_str()).collect();
        assert_eq!(
            said,
            vec![
                "Ana stopped Mo's agents at 2026-11-11 09:30 UTC.",
                "Ana stopped the team agent at 2026-11-11 09:31 UTC.",
                "Lee stopped the connector gmail at 2026-11-11 09:32 UTC.",
                "Ana paused Mo's agent at 2026-11-11 09:33 UTC: nothing new starts for it.",
            ]
        );
        assert!(alerts.iter().all(|a| a["fix"] == "agents"), "{alerts:?}");
        assert!(stop_alerts(&json!({ "on": [], "paused": [] }), &name_of).is_empty());
    }
}
