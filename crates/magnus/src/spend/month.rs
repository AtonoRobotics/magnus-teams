//! What was spent in a calendar month (UTC), and on what. A run's own
//! model calls are its `usd`; a sub-agent's are in its own run, so summing
//! every run's `usd` counts each dollar once. A run counts in the month it
//! started, as the team's limits count it.

use crate::state::State;
use chrono::{DateTime, Datelike, TimeZone, Utc};
use std::collections::BTreeMap;

/// The month `at` is in, as `YYYY-MM`.
pub fn key(at: DateTime<Utc>) -> String {
    format!("{:04}-{:02}", at.year(), at.month())
}

/// The first moment of a `YYYY-MM` month and of the month after it.
pub fn bounds(month: &str) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let (y, m) = month.split_once('-')?;
    let (y, m): (i32, u32) = (y.parse().ok()?, m.parse().ok()?);
    let start = Utc.with_ymd_and_hms(y, m, 1, 0, 0, 0).single()?;
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    Some((start, Utc.with_ymd_and_hms(ny, nm, 1, 0, 0, 0).single()?))
}

/// Who or what did the work: a person's own agent, or the team agent of a
/// workspace. A sub-agent's work is its parent's agent's.
pub fn agent_of(s: &State, run: &str) -> String {
    match s.authority(run).and_then(|t| s.team.run_of(&t.task_id)) {
        Some(t) => format!("team:{}", t.workspace),
        None => "personal".to_string(),
    }
}

/// The project a run's work belongs to: its top task's, else its team
/// channel's.
fn project_of(s: &State, run: &str) -> Option<(String, String)> {
    let task = s.authority(run)?;
    if let Some(p) = &task.project {
        return Some((p.id.clone(), p.name.clone()));
    }
    let t = s.team.run_of(&task.task_id)?;
    let w = s.team.workspace(&t.workspace)?;
    let id = w.channels.get(&t.channel)?.project.clone()?;
    let name = w.projects.get(&id).map(|p| p.name.clone()).unwrap_or_default();
    Some((id, name))
}

/// A month's spend, in all and broken down.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Spent {
    pub total_usd: f64,
    pub by_person: BTreeMap<String, f64>,
    pub by_agent: BTreeMap<String, f64>,
    pub by_model: BTreeMap<String, f64>,
    /// Project id to (its name, dollars).
    pub by_project: BTreeMap<String, (String, f64)>,
}

/// Everything spent in the month.
pub fn spent(s: &State, month: &str) -> Spent {
    let mut out = Spent::default();
    let Some((start, end)) = bounds(month) else { return out };
    for r in s.runs.values().filter(|r| r.created_at >= start && r.created_at < end) {
        let usd = super::calls::legacy(s, r);
        out.total_usd += usd;
        *out.by_person.entry(r.person.clone()).or_default() += usd;
        *out.by_agent.entry(agent_of(s, &r.id)).or_default() += usd;
        *out.by_model.entry(format!("{}/{}", r.config.adapter, r.config.model)).or_default() += usd;
        let (id, name) = project_of(s, &r.id).unwrap_or_default();
        let e = out.by_project.entry(id).or_insert((name, 0.0));
        e.1 += usd;
    }
    for c in s.spend.calls.values().filter(|c| c.month == month) {
        out.total_usd += c.usd;
        *out.by_person.entry(c.person.clone()).or_default() += c.usd;
        *out.by_agent.entry(c.workspace.as_ref().map(|w| format!("team:{w}")).unwrap_or_else(|| "personal".into())).or_default() += c.usd;
        *out.by_model.entry(c.model.clone()).or_default() += c.usd;
        let (id, name) = project_of(s, &c.run).unwrap_or_default();
        out.by_project.entry(id).or_insert((name, 0.0)).1 += c.usd;
    }
    out
}

/// What the server and one person spent in the month, without the rest of
/// the breakdown: what a run's start reads.
pub fn spent_by(s: &State, month: &str, person: &str) -> (f64, f64) {
    let spent = spent(s, month);
    (spent.total_usd, spent.by_person.get(person).copied().unwrap_or_default())
}
