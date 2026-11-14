//! Spend limits: a workspace owner caps what the team agent spends in a
//! calendar month, for the whole workspace and for one channel. Work that
//! would start past a limit does not start, and work that starts is given
//! no more budget than is left once what open team runs may still spend is
//! held for them, so two starts cannot both claim the same remainder. The
//! agent loop rechecks the limits before each model call, so a limit holds
//! even mid-task, when it is lowered or overrun.
use super::chat::Chat;
use super::routes::{has_channel, member, send, Reply, K};
use super::{Change, Role, Workspace};
use crate::api::err;
use crate::kernel::{Kernel, NewRun};
use crate::state::State;
use axum::extract::{Path, State as Ax};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use chrono::{DateTime, Datelike, TimeZone, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;

/// The largest monthly limit an owner can set, in dollars: past it, a typo.
pub const MAX_LIMIT_USD: f64 = 1_000_000.0;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/workspaces/{ws}/limit", post(set_limit))
}

/// What the team agent spent this month in a workspace: in all, and by channel.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Spent {
    pub total_usd: f64,
    pub channels_usd: BTreeMap<String, f64>,
}

/// The first moment of the calendar month `now` is in, in UTC.
fn month_start(now: DateTime<Utc>) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0).single().unwrap_or(now)
}

/// One team start at a time: what is left is read, and the run that claims
/// it recorded, before the next start reads it.
static STARTING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Team work refused at its start because a limit is spent, or held for
/// open runs; the words say which limit.
#[derive(Debug, Clone, PartialEq)]
pub struct TeamLimited(pub String);

impl std::fmt::Display for TeamLimited {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TeamLimited {}

/// What the workspace's team runs spent since the month began, and, when
/// `held`, what those still open may yet spend: each one's unspent budget.
/// A sub-agent's spend is already in its parent's, and its budget is carved
/// from the parent's, so it is counted once.
fn tally(s: &State, ws: &str, now: DateTime<Utc>, held: bool) -> Spent {
    let since = month_start(now);
    let mut out = Spent::default();
    for r in s.team.runs_of(ws).into_iter().filter(|r| !r.sub) {
        let Some(run) = s.tasks.get(&r.task).and_then(|t| s.runs.get(&t.run_id)) else { continue };
        if run.created_at < since {
            continue;
        }
        let unspent = if held && !run.status.terminal() { (run.config.budget.max_usd - run.spent_usd()).max(0.0) } else { 0.0 };
        let usd = run.spent_usd() + unspent;
        out.total_usd += usd;
        *out.channels_usd.entry(r.channel.clone()).or_default() += usd;
    }
    out
}

/// What the workspace's team runs spent since the month began.
pub fn spent_this_month(s: &State, ws: &str, now: DateTime<Utc>) -> Spent {
    tally(s, ws, now, false)
}

/// What the month's team work has spent or may still spend: the spend, and
/// the unspent budget of every team run not yet ended (a paused one too, as
/// it may resume). This is what a new start is checked against.
pub fn committed_this_month(s: &State, ws: &str, now: DateTime<Utc>) -> Spent {
    tally(s, ws, now, true)
}

/// Why a team run, or a sub-agent of one, stops before its next model
/// call: its workspace's or its channel's limit is spent. A sub-agent is
/// held to the limits of the run at the top of its parents.
pub fn stop_partway(s: &State, run: &str, now: DateTime<Utc>) -> Option<(String, Value)> {
    let top = s.authority(run)?;
    let t = s.team.run_of(&top.task_id)?;
    let w = s.team.workspace(&t.workspace)?;
    let spent = spent_this_month(s, &t.workspace, now);
    let why = refusal(w, &spent, &t.channel)?;
    let channel_limit = w.chat.channel_limits_usd.get(&t.channel).copied();
    let channel_spent = spent.channels_usd.get(&t.channel).copied().unwrap_or_default();
    let (which, spent_usd, limit_usd) = match channel_limit {
        Some(l) if channel_spent >= l => ("channel", channel_spent, l),
        _ => ("workspace", spent.total_usd, w.chat.limit_usd.unwrap_or_default()),
    };
    let details = json!({
        "which": which, "workspace": t.workspace, "channel": t.channel, "spent_usd": spent_usd, "limit_usd": limit_usd, "can_ask": false, "pending": false,
    });
    Some((why, details))
}

/// How much more the agent may spend on work in this channel this month,
/// if any limit applies: the tighter of the workspace's and the channel's.
pub fn left_usd(w: &Workspace, spent: &Spent, channel: &str) -> Option<f64> {
    let whole = w.chat.limit_usd.map(|l| l - spent.total_usd);
    let here = w.chat.channel_limits_usd.get(channel).map(|l| l - spent.channels_usd.get(channel).copied().unwrap_or_default());
    match (whole, here) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
    .map(|l| l.max(0.0))
}

/// The workspace's limits and this month's spend, as the client shows them.
pub fn summary(w: &Workspace, spent: &Spent) -> Value {
    let channels: BTreeMap<&String, Value> = w
        .channels
        .keys()
        .filter_map(|id| {
            let limit = w.chat.channel_limits_usd.get(id).copied();
            let usd = spent.channels_usd.get(id).copied().unwrap_or_default();
            (limit.is_some() || usd > 0.0).then(|| (id, json!({ "spent_usd": usd, "limit_usd": limit })))
        })
        .collect();
    json!({ "spent_usd": spent.total_usd, "limit_usd": w.chat.limit_usd, "channels": channels })
}

/// Why work in this channel cannot start, when a limit is spent.
pub fn refusal(w: &Workspace, spent: &Spent, channel: &str) -> Option<String> {
    let left = left_usd(w, spent, channel)?;
    if left > 0.0 {
        return None;
    }
    let channel_spent = w.chat.channel_limits_usd.get(channel).is_some_and(|l| spent.channels_usd.get(channel).copied().unwrap_or_default() >= *l);
    Some(if channel_spent {
        "This channel has reached its spend limit for the month. A workspace owner can raise it.".to_string()
    } else {
        "This workspace has reached its spend limit for the month. A workspace owner can raise it.".to_string()
    })
}

impl Kernel {
    /// The budget left for new team work in a channel this month, if
    /// limited, once open team runs' unspent budgets are held for them; an
    /// error says why nothing is left.
    pub(super) fn team_budget_left(&self, ws: &str, channel: &str) -> Result<Option<f64>, String> {
        self.read(|s| {
            let Some(w) = s.team.workspace(ws) else { return Ok(None) };
            let committed = committed_this_month(s, ws, Utc::now());
            match refusal(w, &committed, channel) {
                Some(why) => Err(why),
                None => Ok(left_usd(w, &committed, channel)),
            }
        })
    }

    /// Starts team work within its workspace's and channel's limits: none
    /// when nothing is left, and no more budget than is left otherwise.
    /// Starts take turns, so the remainder one reads is never one another
    /// start has claimed but not yet recorded.
    pub(super) async fn start_team_run(self: &Arc<Self>, actor: &str, person: &str, text: String, mut o: NewRun) -> anyhow::Result<(String, String)> {
        let Some((ws, channel)) = o.team.as_ref().map(|t| (t.workspace.clone(), t.channel.clone())) else { anyhow::bail!("team work needs its team thread") };
        let _one = STARTING.lock().await;
        let left = self.team_budget_left(&ws, &channel).map_err(|why| anyhow::Error::new(TeamLimited(why)))?;
        if let Some(usd) = left {
            let b = o.budget.take().unwrap_or_else(|| self.cfg.budget.clone());
            o.budget = Some(crate::config::Budget { max_usd: b.max_usd.min(usd), ..b });
        }
        self.create_run(actor, person, text, vec![], o).await
    }
}

#[derive(Deserialize)]
struct Limit {
    /// None lifts the limit.
    #[serde(default)]
    usd: Option<f64>,
    /// A channel's limit, or the workspace's when absent.
    #[serde(default)]
    channel: Option<String>,
}

/// Only an owner sets a limit, for the workspace or one of its channels.
async fn set_limit(Ax(k): K, h: HeaderMap, Path(ws): Path<String>, Json(b): Json<Limit>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Owner).await);
    if b.usd.is_some_and(|u| !u.is_finite() || !(0.0..=MAX_LIMIT_USD).contains(&u)) {
        return err(StatusCode::BAD_REQUEST, "a limit is an amount of dollars from 0 up");
    }
    if let Some(c) = &b.channel {
        if !has_channel(&k, &ws, c, &p.id) {
            return err(StatusCode::NOT_FOUND, "no such channel");
        }
    }
    ok_or_return!(send(&k, &p, Change::Chat { workspace: ws, chat: Chat::SpendLimit { channel: b.channel, usd: b.usd } }).await);
    Json(json!({ "ok": true })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limited(whole: Option<f64>, channel: Option<f64>) -> Workspace {
        let mut w = Workspace::default();
        w.chat.limit_usd = whole;
        if let Some(c) = channel {
            w.chat.channel_limits_usd.insert("c".into(), c);
        }
        w
    }

    fn spent(total: f64, c: f64) -> Spent {
        Spent { total_usd: total, channels_usd: BTreeMap::from([("c".to_string(), c)]) }
    }

    #[test]
    fn no_limit_leaves_work_unlimited() {
        assert_eq!(left_usd(&limited(None, None), &spent(500.0, 500.0), "c"), None);
        assert_eq!(refusal(&limited(None, None), &spent(500.0, 500.0), "c"), None);
    }

    #[test]
    fn the_tighter_limit_holds() {
        assert_eq!(left_usd(&limited(Some(100.0), Some(30.0)), &spent(50.0, 10.0), "c"), Some(20.0));
        assert_eq!(left_usd(&limited(Some(100.0), Some(30.0)), &spent(95.0, 10.0), "c"), Some(5.0));
        assert_eq!(left_usd(&limited(Some(100.0), Some(30.0)), &spent(95.0, 10.0), "other"), Some(5.0), "another channel is still under the workspace's limit");
    }

    #[test]
    fn a_spent_limit_refuses_and_says_which() {
        let channel = refusal(&limited(Some(100.0), Some(30.0)), &spent(40.0, 30.0), "c").unwrap_or_default();
        assert!(channel.contains("This channel"), "{channel}");
        let whole = refusal(&limited(Some(100.0), None), &spent(100.5, 1.0), "c").unwrap_or_default();
        assert!(whole.contains("This workspace"), "{whole}");
        assert_eq!(left_usd(&limited(Some(100.0), None), &spent(120.0, 1.0), "c"), Some(0.0), "an overrun reads as nothing left");
    }

    #[test]
    fn a_month_starts_on_its_first_day() {
        let now = Utc.with_ymd_and_hms(2026, 11, 10, 15, 4, 5).single().unwrap_or_default();
        assert_eq!(month_start(now), Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).single().unwrap_or_default());
    }

    #[test]
    fn a_limit_is_set_and_lifted_by_its_change() {
        let mut w = Workspace::default();
        super::super::chat::apply(&mut w, &Chat::SpendLimit { channel: None, usd: Some(50.0) });
        super::super::chat::apply(&mut w, &Chat::SpendLimit { channel: Some("c".into()), usd: Some(10.0) });
        assert_eq!((w.chat.limit_usd, w.chat.channel_limits_usd.get("c").copied()), (Some(50.0), Some(10.0)));
        super::super::chat::apply(&mut w, &Chat::SpendLimit { channel: Some("c".into()), usd: None });
        super::super::chat::apply(&mut w, &Chat::SpendLimit { channel: None, usd: None });
        assert_eq!((w.chat.limit_usd, w.chat.channel_limits_usd.len()), (None, 0));
    }
}
