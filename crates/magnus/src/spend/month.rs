//! What was spent in a calendar month (UTC), and on what: read from the
//! money ledger (`book`), where each dollar is written once, in the month
//! it was spent, and stays when the run that spent it is trashed or its
//! person erased.

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

/// A project's name, by its id: the person's own or a workspace's.
fn project_name(s: &State, id: &str) -> String {
    if let Some(p) = s.library.projects().get(id) {
        return p.name.clone();
    }
    s.team.all().find_map(|w| w.projects.get(id).map(|p| p.name.clone())).unwrap_or_default()
}

/// Everything spent in the month, from the ledger (`book`). Spend that is
/// no one person's (the server's own, a workspace's, an erased person's)
/// is in the total and the agent's and model's rows, not by person.
pub fn spent(s: &State, month: &str) -> Spent {
    let mut out = Spent::default();
    for (l, usd) in s.spend.book.month(month) {
        out.total_usd += usd;
        if !l.person.is_empty() && l.person != super::book::ERASED {
            *out.by_person.entry(l.person.clone()).or_default() += usd;
        }
        *out.by_agent.entry(l.agent.clone()).or_default() += usd;
        *out.by_model.entry(l.model.clone()).or_default() += usd;
        let e = out.by_project.entry(l.project.clone()).or_insert_with(|| (project_name(s, &l.project), 0.0));
        e.1 += usd;
    }
    out
}

/// What the server and one person spent in the month, without the rest of
/// the breakdown: what every limit reads.
pub fn spent_by(s: &State, month: &str, person: &str) -> (f64, f64) {
    s.spend.book.month(month).fold((0.0, 0.0), |(all, theirs), (l, usd)| (all + usd, theirs + if l.person == person { usd } else { 0.0 }))
}
