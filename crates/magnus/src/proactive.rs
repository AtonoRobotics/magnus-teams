//! The proactive loop's settings and hours (behavior step 7, #237): each
//! person's working and quiet hours, review cadence and interruption
//! budget, resolved from their preferences over the deployment's config.
//! Anything unparsable falls back to the default; settings never break a
//! review.

use crate::tools::Label;
use chrono::{DateTime, TimeZone, Timelike, Utc};
use serde_json::Value;
use std::sync::Arc;

/// A failed poll mark waits this long times its attempt before it is emitted again.
const MARK_RETRY_STEP_MS: u64 = 100;

/// The ended task that woke a review: its ID, its outcome line, and the
/// taint it ended with, which the review starts with (#383, #390).
pub(crate) struct Ended {
    pub task: String,
    pub outcome: String,
    pub label: Label,
}

/// Minutes since midnight for a "HH:MM" time, in the person's timezone.
fn at(hhmm: &str) -> Option<u32> {
    let (h, m) = hhmm.split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then(|| h * 60 + m)
}

fn window(v: &serde_json::Value, start: &str, end: &str) -> (u32, u32) {
    let s = v.get("start").and_then(serde_json::Value::as_str).and_then(at);
    let e = v.get("end").and_then(serde_json::Value::as_str).and_then(at);
    match (s, e) {
        (Some(s), Some(e)) => (s, e),
        _ => (at(start).unwrap(), at(end).unwrap()),
    }
}

/// The person's timezone from preferences, UTC without one.
pub fn tz_of(prefs: &serde_json::Value) -> chrono_tz::Tz {
    prefs.get("timezone").and_then(serde_json::Value::as_str).and_then(|z| z.parse().ok()).unwrap_or(chrono_tz::Tz::UTC)
}

/// The person's resolved loop settings.
pub struct Settings {
    pub tz: chrono_tz::Tz,
    pub working: (u32, u32),
    pub quiet: (u32, u32),
    pub every_minutes: u32,
    pub per_day: u32,
}

/// Resolve settings: preferences over the `[proactive]` config, defaults
/// where either is missing or unparsable. Working 9 to 5, quiet 10pm to
/// 7am, weekly cadence floor of 5 minutes.
pub fn settings(cfg: &crate::config::Proactive, prefs: &serde_json::Value) -> Settings {
    let num = |k: &str, dflt: u32| prefs.get(k).and_then(serde_json::Value::as_u64).map(|n| n as u32).filter(|n| *n > 0).unwrap_or(dflt);
    Settings {
        tz: prefs.get("timezone").and_then(serde_json::Value::as_str).and_then(|z| z.parse().ok()).unwrap_or(chrono_tz::Tz::UTC),
        working: prefs.get("working_hours").map(|w| window(w, "09:00", "17:00")).unwrap_or((540, 1020)),
        quiet: prefs.get("quiet_hours").map(|w| window(w, "22:00", "07:00")).unwrap_or((1320, 420)),
        every_minutes: num("review_every_minutes", cfg.review_every_minutes).max(5),
        per_day: num("interruptions_per_day", cfg.interruptions_per_day),
    }
}

/// Minutes since midnight where the person is.
pub fn minutes_at(now: DateTime<Utc>, tz: chrono_tz::Tz) -> u32 {
    let local = now.with_timezone(&tz);
    local.hour() * 60 + local.minute()
}

/// Whether minute-of-day `m` falls in the window, which may cross midnight
/// (an end at or before its start runs into the next day).
pub fn in_window(m: u32, (start, end): (u32, u32)) -> bool {
    if end > start {
        m >= start && m < end
    } else {
        m >= start || m < end
    }
}

/// The person's calendar day, for the daily interruption budget.
pub fn day_key(now: DateTime<Utc>, tz: chrono_tz::Tz) -> String {
    now.with_timezone(&tz).date_naive().to_string()
}

/// When the current quiet hours end, for delaying an interruption's
/// notification until then. None when it is not quiet now.
pub fn quiet_ends_at(now: DateTime<Utc>, tz: chrono_tz::Tz, quiet: (u32, u32)) -> Option<DateTime<Utc>> {
    if !in_window(minutes_at(now, tz), quiet) {
        return None;
    }
    let local = now.with_timezone(&tz);
    let mut day = local.date_naive();
    if quiet.1 <= quiet.0 && local.hour() * 60 + local.minute() >= quiet.0 {
        day = day.succ_opt()?;
    }
    let naive = day.and_hms_opt(quiet.1 / 60, quiet.1 % 60, 0)?;
    Some(tz.from_local_datetime(&naive).earliest()?.with_timezone(&Utc))
}

/// The window's next start, for holding an interruption's notification
/// until working hours. None when in the window now.
pub fn window_next_start(now: DateTime<Utc>, tz: chrono_tz::Tz, window: (u32, u32)) -> Option<DateTime<Utc>> {
    if in_window(minutes_at(now, tz), window) {
        return None;
    }
    let local = now.with_timezone(&tz);
    let mut day = local.date_naive();
    if window.1 > window.0 && local.hour() * 60 + local.minute() >= window.0 {
        day = day.succ_opt()?;
    }
    let naive = day.and_hms_opt(window.0 / 60, window.0 % 60, 0)?;
    Some(tz.from_local_datetime(&naive).earliest()?.with_timezone(&Utc))
}

/// A coming-up schedule's wake reason, carrying its id for the once-a-day
/// mark the log keeps (#237).
pub fn schedule_reason(id: &str) -> String {
    format!("schedule {id} coming up")
}

/// A due commitment's wake reason, carrying its id likewise.
pub fn commitment_reason(id: &str) -> String {
    format!("commitment {id} due")
}

/// A stale approval's wake reason, carrying its item likewise.
pub fn approval_reason(id: &str) -> String {
    format!("approval {id} waiting")
}

impl crate::kernel::Kernel {
    /// Wake review runs whose time has come: cadence in working hours, once
    /// overnight, coming-up schedules, due commitments and stale approvals
    /// (#237). Called from the sweep.
    pub async fn review_due(self: &Arc<Self>, now: DateTime<Utc>) {
        if !self.cfg.proactive.on {
            return;
        }
        // Pushes that landed inside their gap poll once it passes.
        self.push_due(now).await;
        // Subscriptions at providers kept to what they should be.
        self.push_subs_due(now);
        // Everyone the server keeps: config people and those invited since.
        let people: Vec<String> = self.everyone().into_iter().map(|(id, _)| id).collect();
        for person in people {
            let start = crate::halts::Subject::new_work(&person, false);
            if self.read(|s| crate::halts::may_proceed(s, &start, crate::halts::Phase::Start).is_some() || s.reviewing(&person)) {
                continue;
            }
            let prefs = self.read(|s| s.preferences.get(&person).cloned().unwrap_or(serde_json::Value::Null));
            let st = settings(&self.cfg.proactive, &prefs);
            let m = minutes_at(now, st.tz);
            let today = day_key(now, st.tz);
            let (last, overnight) = self.read(|s| (s.reviews_last.get(&person).cloned(), s.reviews_overnight.get(&person).cloned()));
            // One sweep starts at most one review per person: an event
            // first (it is marked, so the cadence cannot starve it, and it
            // is the more urgent), else the periodic one (#381).
            let event: Option<(String, Option<Ended>)> = self.read(|s| {
                for sc in s.schedules.values().filter(|sc| sc.person == person && sc.paused.is_none()) {
                    if sc.next_due().is_some_and(|d| d > now && d <= now + chrono::Duration::minutes(st.every_minutes as i64))
                        && !s.wake_marks.get(&format!("{person}:schedule {}", sc.id)).is_some_and(|d| d == &today)
                    {
                        return Some((schedule_reason(&sc.id), None));
                    }
                }
                for c in s.commitments.values().filter(|c| c.person == person && c.status == "open") {
                    if c.by.as_deref().and_then(crate::commitments::due).is_some_and(|d| d > now && d <= now + chrono::Duration::hours(24))
                        && !s.wake_marks.get(&format!("{person}:commitment {}", c.id)).is_some_and(|d| d == &today)
                    {
                        return Some((commitment_reason(&c.id), None));
                    }
                }
                for i in s.items.values().filter(|i| i.person == person && i.kind == "approval" && i.resolution.is_none()) {
                    if now.signed_duration_since(i.created_at).num_minutes() >= 30 && !s.wake_marks.get(&format!("{person}:approval {}", i.id)).is_some_and(|d| d == &today) {
                        return Some((approval_reason(&i.id), None));
                    }
                }
                // The backstop (#386): a task that ended while a review
                // was active had its fast wake dropped. The newest such
                // task with no wake mark, ended over thirty seconds ago
                // so the fast wake had its chance, gets its review from
                // this sweep. The mark lands from the reason when the
                // review starts, exactly as for the fast wake.
                if let Some((id, verb, outcome, label)) = s
                    .tasks
                    .values()
                    .filter(|t| t.person == person && s.end_wakes_review(&t.run_id))
                    .filter_map(|t| {
                        let r = s.runs.get(&t.run_id)?;
                        let verb = match r.status {
                            crate::state::Status::Completed => "done",
                            crate::state::Status::Failed => "failed",
                            _ => return None,
                        };
                        let ended = r.ended_at?;
                        if now.signed_duration_since(ended).num_seconds() < 30 {
                            return None;
                        }
                        if s.wake_marks.contains_key(&format!("{person}:task {}", t.task_id)) {
                            return None;
                        }
                        let label = Label { untrusted: r.untrusted, private: r.private };
                        Some((ended, t.task_id.clone(), verb, r.summary.clone().unwrap_or_default(), label))
                    })
                    .max_by_key(|(ended, _, _, _, _)| *ended)
                    .map(|(_, id, verb, outcome, label)| (id, verb, outcome, label))
                {
                    return Some((format!("task {id} {verb}"), Some(Ended { task: id, outcome, label })));
                }
                None
            });
            if let Some((reason, ended)) = event {
                self.start_review(&person, reason, ended).await;
            } else if in_window(m, st.working) && last.is_none_or(|t| now.signed_duration_since(t).num_minutes() >= st.every_minutes as i64) {
                self.start_review(&person, "cadence".into(), None).await;
            } else if !in_window(m, st.working) && overnight.as_deref() != Some(today.as_str()) {
                self.start_review(&person, "overnight".into(), None).await;
            }
        }
    }

    /// Poll watched reads whose interval has passed (#406). Called
    /// from the sweep beside the review wakes. Each due watch starts
    /// one minimal polling run; its answer diffs through the same hook
    /// as any read, so items nobody reads still wake. Polls run even
    /// while a review is active: a cheap read waits for nothing. Only
    /// watches the person can currently use are polled: the effective
    /// connector list (a turned-off app is skipped), and only when
    /// they are connected and their sign-in has not lapsed (#418). A
    /// run that cannot read is never started.
    pub async fn poll_due(self: &Arc<Self>, now: DateTime<Utc>) {
        if !self.cfg.proactive.on {
            return;
        }
        let people: Vec<String> = self.everyone().into_iter().map(|(id, _)| id).collect();
        for person in people {
            // A poll is work done for the person through their sign-in: the
            // server's switch, theirs, their agent's, the app's, and their
            // account turned off each hold it (decisions/2026-11-13-one-authorised-read.md).
            let held = |app: Option<&str>| self.hold(&crate::halts::Subject::person_app(&person, app), crate::halts::Phase::Call).is_some();
            if held(None) {
                continue;
            }
            let watches: Vec<(String, crate::config::Watch)> = self
                .connectors()
                .iter()
                .filter(|c| self.connected(c, &person) && !self.lapsed(c, &person) && !held(Some(&c.name)))
                .flat_map(|c| c.watch.iter().map(|w| (c.name.clone(), w.clone())))
                .filter(|(_, w)| w.poll_minutes.is_some())
                .collect();
            for (app, w) in watches {
                let minutes = w.poll_minutes.unwrap_or(0).max(1) as i64;
                let due =
                    self.read(|s| s.watch_last_polled.get(&(person.clone(), app.clone(), w.tool.clone())).is_none_or(|t| now.signed_duration_since(*t).num_minutes() >= minutes));
                if due {
                    self.start_poll(&person, &app, &w).await;
                }
            }
        }
    }

    /// Emit a poll mark that survives a hiccuping event store (#438):
    /// the store is Postgres, and a lost mark re-polls every sweep.
    /// Three attempts with a short backoff; the caller warns loudly
    /// on final failure instead of silently dropping the mark.
    pub(crate) async fn emit_poll_mark<F, Fut>(mut emit: F) -> anyhow::Result<()>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<()>>,
    {
        let mut attempt = 0;
        loop {
            match emit().await {
                Ok(()) => return Ok(()),
                Err(err) if attempt < 2 => {
                    attempt += 1;
                    tracing::debug!("poll mark emit failed, retrying ({attempt}/2): {err:#}");
                    tokio::time::sleep(std::time::Duration::from_millis(MARK_RETRY_STEP_MS * attempt as u64)).await;
                }
                Err(err) => return Err(err),
            }
        }
    }

    /// Start one poll: the kernel calls the watched read itself, once,
    /// with the watch's arguments (`watch_reads::poll_read`). No model
    /// chooses the arguments and no task is made. The read's own diff
    /// wakes when there is news. The clock restarts when the poll starts,
    /// not when it answers, so a read that fails waits for its next
    /// interval instead of retrying every sweep; the failure is logged.
    pub(crate) async fn start_poll(self: &Arc<Self>, person: &str, app: &str, watch: &crate::config::Watch) {
        if let Err(e) = Self::emit_poll_mark(|| async {
            self.emit("kernel", crate::events::Ev::WatchPolled { person: person.into(), app: app.into(), tool: watch.tool.clone() }).await.map(|_| ())
        })
        .await
        {
            tracing::warn!("watch poll mark lost for {person} {app} after retries, polling again next sweep: {e:#}");
            return;
        }
        let (k, person, app, watch) = (self.clone(), person.to_string(), app.to_string(), watch.clone());
        tokio::spawn(async move {
            // One retry for a transient error (#419), then the next interval.
            for attempt in 1..=crate::watch_reads::POLL_READ_ATTEMPTS {
                match k.poll_read(&person, &app, &watch, Utc::now()).await {
                    Ok(()) => return,
                    Err(e) if attempt < crate::watch_reads::POLL_READ_ATTEMPTS => {
                        tracing::debug!("watch poll of {app}__{} for {person} failed, reading again: {e}", watch.tool);
                        tokio::time::sleep(crate::watch_reads::POLL_RETRY_WAIT).await;
                    }
                    Err(e) => tracing::warn!("watch poll of {app}__{} for {person} failed: {e}", watch.tool),
                }
            }
        });
    }

    /// Wake one review for an event (a task done or failed), unless one is
    /// already going or this task already woke one (#237, #383). The wake
    /// used to skip when any review had woken within five minutes, which
    /// silently dropped failures after a cadence review or a boot; a
    /// per-task mark is the precise dedup (a task ends once).
    pub async fn review_wake(self: &Arc<Self>, person: &str, reason: String, ended: Option<Ended>) -> bool {
        if !self.cfg.proactive.on {
            return false;
        }
        if self.read(|s| s.reviewing(person) || ended.as_ref().is_some_and(|e| s.wake_marks.contains_key(&format!("{person}:task {}", e.task)))) {
            return false;
        }
        self.start_review(person, reason, ended).await
    }

    /// Wake one review for new items in a connected app, unless one is
    /// already going (#369). True when a review started: the caller
    /// advances its baseline only then, so a dropped wake retries on
    /// the next read. The reason carries the count only; the review
    /// pulls the items itself, tainting normally through the boundary.
    pub async fn review_wake_items(self: &Arc<Self>, person: &str, app: &str, n: usize) -> bool {
        if !self.cfg.proactive.on {
            return false;
        }
        if self.read(|s| s.reviewing(person)) {
            return false;
        }
        self.start_review(person, format!("{n} new in {app}"), None).await
    }

    /// A watched tool answered for the person: diff its items against
    /// the last two answers, waking a review for new ones (#369). The
    /// first sighting seeds silently; every read after advances the
    /// window, news or not, so the window tracks reads, not wakes
    /// (#414). A dropped wake advances nothing, so it retries on the
    /// next read instead of losing the items. Two answers, not all
    /// history (#409): no cap to evict from, so an unchanged answer
    /// diffs empty however large it is, and memory stays two answers
    /// deep. Every answer also restarts the poll clock (#413): a fresh
    /// read postpones the next poll, whoever made it.
    /// `watches` are those of the app the call went through, the
    /// catalog's included: a catalog app's come with its manifest.
    pub(crate) async fn note_watched(self: &Arc<Self>, person: &str, app: &str, remote: &str, watches: &[crate::config::Watch], data: &Value) {
        for w in watches.iter().filter(|w| w.tool == remote) {
            let Some(mut ids) = crate::watch_reads::ids(data, &w.items, &w.id) else {
                tracing::warn!("{app}__{remote} answered with no items at {:?}: its watch does not match the answer", w.items);
                continue;
            };
            ids.sort();
            ids.dedup();
            // A fresh read postpones the next poll (#413). The start mark
            // stays the bound for polls whose reads never answer.
            if let Err(e) = Self::emit_poll_mark(|| async {
                self.emit(&format!("person:{person}"), crate::events::Ev::WatchPolled { person: person.into(), app: app.into(), tool: remote.into() }).await.map(|_| ())
            })
            .await
            {
                tracing::warn!("watch answer mark lost for {person} {app}, polling on the old schedule: {e:#}");
            }
            let window = self.read(|s| s.watched_seen.get(&(person.to_string(), app.to_string(), remote.to_string())).cloned());
            let Some(window) = window else {
                let _ =
                    self.emit(&format!("person:{person}"), crate::events::Ev::WatchedSeen { person: person.into(), app: app.into(), tool: remote.into(), ids, full: true }).await;
                continue;
            };
            let fresh: Vec<&String> = ids.iter().filter(|id| !window.iter().any(|w| w.contains(*id))).collect();
            if fresh.is_empty() {
                // No news, but the read still counts: the window is the
                // last two reads, whatever they held (#414).
                let _ =
                    self.emit(&format!("person:{person}"), crate::events::Ev::WatchedSeen { person: person.into(), app: app.into(), tool: remote.into(), ids, full: true }).await;
                continue;
            }
            let n = fresh.len();
            if self.review_wake_items(person, app, n).await {
                let _ =
                    self.emit(&format!("person:{person}"), crate::events::Ev::WatchedSeen { person: person.into(), app: app.into(), tool: remote.into(), ids, full: true }).await;
            }
        }
    }

    /// Start the review run itself: the review instruction over the
    /// person's state, on the per-task budget, which its Auto Review calls
    /// are charged to as well (#237, spec rev 133).
    async fn start_review(self: &Arc<Self>, person: &str, reason: String, ended: Option<Ended>) -> bool {
        let context = self.review_context(person, ended.as_ref()).await;
        let text = format!("Review the person's state ({reason}).\n\n{context}");
        let budget = crate::task_budget::for_wake(&self.cfg.budget);
        match self
            .create_run(
                "review",
                person,
                text,
                vec![],
                crate::kernel::NewRun {
                    budget: Some(budget),
                    // The outcome can quote what the task read: the review
                    // starts with the ended run's taint, never clean (#390).
                    label: ended.map(|e| e.label),
                    ..Default::default()
                },
            )
            .await
        {
            Ok((task, run)) => {
                let _ = self.emit("kernel", crate::events::Ev::ReviewWoken { run, task, person: person.into(), reason }).await;
            }
            Err(e) => {
                tracing::warn!("review wake for {person}: {e:#}");
                return false;
            }
        }
        true
    }

    /// The review's context: the ended task that woke it, if any, then
    /// unfinished work, schedules, approvals, disagreements and
    /// commitments (#237, #383).
    async fn review_context(&self, person: &str, ended: Option<&Ended>) -> String {
        let (tasks, schedules, approvals, disagreements, commitments) = self.read(|s| {
            let tasks = s
                .tasks
                .values()
                .filter(|t| t.person == person && !t.review && !t.poll && s.runs.get(&t.run_id).is_some_and(|r| !r.status.terminal()))
                .map(|t| (t.person.clone(), t.seq))
                .collect::<Vec<_>>();
            let schedules = s
                .schedules
                .values()
                .filter(|sc| sc.person == person)
                .map(|sc| {
                    format!(
                        "{}: {}{}",
                        sc.id,
                        sc.paused
                            .as_deref()
                            .map(|r| format!("paused ({r})"))
                            .unwrap_or_else(|| format!("next {}", sc.next_due().map(|d| d.to_rfc3339()).unwrap_or_else(|| "unscheduled".into()))),
                        if sc.failures.is_empty() { "" } else { " (failing)" }
                    )
                })
                .collect::<Vec<_>>();
            let approvals = s.items.values().filter(|i| i.person == person && i.kind == "approval" && i.resolution.is_none()).map(|i| i.summary.clone()).collect::<Vec<_>>();
            let disagreements = s
                .disagreements
                .values()
                .filter(|d| d.person == person && d.outcome.is_none())
                .map(|d| format!("{} -> {}", d.position.chars().take(120).collect::<String>(), d.decision))
                .collect::<Vec<_>>();
            let commitments = s
                .commitments
                .values()
                .filter(|c| c.person == person && c.status == "open")
                .map(|c| {
                    format!(
                        "{} (by {}){}",
                        c.id,
                        c.by.as_deref().unwrap_or("no date"),
                        if c.by.as_deref().and_then(crate::commitments::due).is_some_and(|d| d < Utc::now()) { ", OVERDUE" } else { "" }
                    )
                })
                .collect::<Vec<_>>();
            (tasks, schedules, approvals, disagreements, commitments)
        });
        let mut open = vec![];
        if let Ok(words) = self.words(tasks).await {
            open = self.read(|s| {
                s.tasks
                    .values()
                    .filter(|t| t.person == person && !t.review && !t.poll && s.runs.get(&t.run_id).is_some_and(|r| !r.status.terminal()))
                    .map(|t| format!("{}: {}", t.task_id, words.text(t.seq).chars().take(160).collect::<String>()))
                    .collect::<Vec<_>>()
            });
        }
        let section = |title: &str, items: Vec<String>| {
            if items.is_empty() {
                String::new()
            } else {
                format!("{title}:\n{}\n\n", items.iter().take(10).map(|i| format!("- {i}")).collect::<Vec<_>>().join("\n"))
            }
        };
        // The ended task first: the unfinished-work section excludes it,
        // and the review cannot judge the failure without it (#383).
        let woke = ended.map(|e| format!("The task that woke this review:\n- {}: {}\n\n", e.task, e.outcome.chars().take(500).collect::<String>())).unwrap_or_default();
        format!(
            "{woke}{}{}{}{}{}",
            section("Unfinished work", open),
            section("Schedules", schedules),
            section("Approvals waiting", approvals),
            section("Open disagreements", disagreements),
            section("Open commitments", commitments)
        )
        .chars()
        .take(6000)
        .collect()
    }

    /// Now, or when quiet ends while it is quiet: when an observation or
    /// interruption may ask for attention (#237).
    pub fn notify_due(&self, person: &str) -> DateTime<Utc> {
        let prefs = self.read(|s| s.preferences.get(person).cloned().unwrap_or(serde_json::Value::Null));
        let st = settings(&self.cfg.proactive, &prefs);
        let now = Utc::now();
        quiet_ends_at(now, st.tz, st.quiet).unwrap_or(now)
    }

    /// Deliver an interruption, or queue it as an observation: outside
    /// working hours or over budget it waits for the next session; in
    /// quiet hours it posts now and pings when quiet ends (#237). What
    /// the `interrupt` record says back.
    /// Whether the loop may `op` (`suggest`, `interrupt`, `act`) for
    /// this run: person-directed runs are not the loop, so only review
    /// and schedule-originated runs are governed and anything else is
    /// allowed; a governed run needs the policy's leave (#368). Some
    /// reason denies, following the boundary's fail-closed rule.
    pub(crate) fn loop_deny(&self, run: &crate::state::Run, op: &str, tool: Option<&crate::policy::ToolFacts>) -> Option<String> {
        let (task, policies) = self.read(|s| (s.tasks.get(&run.task).cloned(), s.policies.clone()));
        let task = match task {
            Some(t) => t,
            None => return Some("policy cannot evaluate the loop action: the run has no task".into()),
        };
        if !task.review && task.schedule.is_none() {
            return None;
        }
        let person = match self.person(&run.person) {
            Some(p) => p,
            None => return Some("policy cannot evaluate the loop action: the run has no person".into()),
        };
        match crate::policy::loop_allowed(&policies, &person, op, tool, run.untrusted, run.private) {
            Ok(_) => None,
            Err(crate::policy::LoopDeny::Forbidden(r)) | Err(crate::policy::LoopDeny::Broken(r)) => Some(r),
        }
    }

    pub async fn interrupt_for(&self, run: &crate::state::Run, what: &str, why: &str, call_id: &str) -> anyhow::Result<(String, bool)> {
        let prefs = self.read(|s| s.preferences.get(&run.person).cloned().unwrap_or(serde_json::Value::Null));
        let st = settings(&self.cfg.proactive, &prefs);
        let now = Utc::now();
        if let Some(denied) = self.loop_deny(run, "interrupt", None) {
            // Denied like a spent budget: the news waits as an
            // observation when suggestions are allowed, else nothing is
            // queued (#368).
            if self.loop_deny(run, "suggest", None).is_none() {
                let at = quiet_ends_at(now, st.tz, st.quiet).unwrap_or(now);
                let item = self
                    .item(
                        run,
                        "observation",
                        what.to_string(),
                        serde_json::json!({ "why": why, "evidence": "queued: interruptions not permitted", "call_id": call_id, "notify_at": at.to_rfc3339() }),
                    )
                    .await?;
                return Ok((format!("Interruptions are not permitted ({denied}): queued as observation {item} for the next session."), false));
            }
            return Ok((format!("Interruptions and observations are not permitted ({denied}): nothing was queued."), true));
        }
        if !in_window(minutes_at(now, st.tz), st.working) {
            // The fallback queues an observation, so it needs the
            // suggest leave too: without it the denial would leak
            // through here (#402).
            if let Some(denied) = self.loop_deny(run, "suggest", None) {
                return Ok((format!("Outside working hours, and observations are not permitted ({denied}): nothing was queued."), true));
            }
            let at = window_next_start(now, st.tz, st.working).unwrap_or(now);
            let item = self
                .item(
                    run,
                    "observation",
                    what.to_string(),
                    serde_json::json!({ "why": why, "evidence": "queued: outside working hours", "call_id": call_id, "notify_at": at.to_rfc3339() }),
                )
                .await?;
            return Ok((format!("Outside working hours: queued as observation {item}; it pings at {}.", at.with_timezone(&st.tz).format("%H:%M")), false));
        }
        let today = day_key(now, st.tz);
        let n = self.read(|s| s.interrupts.get(&run.person).filter(|(d, _)| *d == today).map(|(_, n)| *n).unwrap_or(0));
        if n >= st.per_day {
            // As above: the fallback is an observation, gated on
            // suggest (#402).
            if let Some(denied) = self.loop_deny(run, "suggest", None) {
                return Ok((format!("Interruption budget spent ({n} today), and observations are not permitted ({denied}): nothing was queued."), true));
            }
            let at = quiet_ends_at(now, st.tz, st.quiet).unwrap_or(now);
            let item = self
                .item(
                    run,
                    "observation",
                    what.to_string(),
                    serde_json::json!({ "why": why, "evidence": "queued: interruption budget spent", "call_id": call_id, "notify_at": at.to_rfc3339() }),
                )
                .await?;
            return Ok((format!("Interruption budget spent ({n} today): queued as observation {item} for the next session."), false));
        }
        let at = quiet_ends_at(now, st.tz, st.quiet).unwrap_or(now);
        let item = self.item(run, "interrupt", what.to_string(), serde_json::json!({ "why": why, "call_id": call_id, "notify_at": at.to_rfc3339() })).await?;
        if at > now {
            Ok((format!("Posted as interruption {item}; it pings when quiet ends at {}.", at.with_timezone(&st.tz).format("%H:%M")), false))
        } else {
            Ok((format!("Delivered as interruption {item}."), false))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Watched ids come from the pointed array's id field; misshapen
    /// answers and id-less items yield nothing, never a wake (#369).
    /// Windows hold across midnight; garbage falls back to defaults.
    #[test]
    fn hours_windows_and_fallbacks() {
        assert!(in_window(600, (540, 1020)) && !in_window(1020, (540, 1020)));
        assert!(in_window(1400, (1320, 420)) && in_window(100, (1320, 420)) && !in_window(500, (1320, 420)));
        let cfg = crate::config::Proactive::default();
        let s = settings(&cfg, &serde_json::json!({ "working_hours": { "start": "xx", "end": "17:00" }, "timezone": "Mars/Olympus", "review_every_minutes": 0 }));
        assert_eq!(((s.working, s.tz, s.every_minutes)), ((540, 1020), chrono_tz::Tz::UTC, 60));
        assert_eq!(day_key(DateTime::parse_from_rfc3339("2026-11-07T06:30:00Z").unwrap().into(), chrono_tz::America::Los_Angeles), "2026-11-06");
        let ends = quiet_ends_at(DateTime::parse_from_rfc3339("2026-11-07T06:30:00Z").unwrap().into(), chrono_tz::Tz::UTC, (1320, 420)).unwrap();
        assert_eq!(ends.to_rfc3339(), "2026-11-07T07:00:00+00:00");
        assert!(quiet_ends_at(DateTime::parse_from_rfc3339("2026-11-07T12:00:00Z").unwrap().into(), chrono_tz::Tz::UTC, (1320, 420)).is_none());
        let work = (540, 1020);
        assert!(window_next_start(DateTime::parse_from_rfc3339("2026-11-07T12:00:00Z").unwrap().into(), chrono_tz::Tz::UTC, work).is_none());
        assert_eq!(
            window_next_start(DateTime::parse_from_rfc3339("2026-11-07T08:00:00Z").unwrap().into(), chrono_tz::Tz::UTC, work).unwrap().to_rfc3339(),
            "2026-11-07T09:00:00+00:00"
        );
        assert_eq!(
            window_next_start(DateTime::parse_from_rfc3339("2026-11-07T18:00:00Z").unwrap().into(), chrono_tz::Tz::UTC, work).unwrap().to_rfc3339(),
            "2026-11-08T09:00:00+00:00"
        );
    }

    /// A poll mark survives a hiccuping event store (#438): transient
    /// failures retry, then a loud error instead of a silent drop.
    #[tokio::test]
    async fn poll_mark_emit_retries_transient_failures() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        // Fail twice, then succeed: three attempts, Ok.
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let r = crate::kernel::Kernel::emit_poll_mark(|| {
            let c = c.clone();
            async move {
                if c.fetch_add(1, Ordering::SeqCst) < 2 {
                    Err(anyhow::anyhow!("the store hiccuped"))
                } else {
                    Ok(())
                }
            }
        })
        .await;
        assert!(r.is_ok(), "the mark never landed: {r:?}");
        assert_eq!(calls.load(Ordering::SeqCst), 3, "transient failures did not retry");
        // Always failing: three attempts, then the error.
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let r = crate::kernel::Kernel::emit_poll_mark(|| {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err(anyhow::anyhow!("the store is down"))
            }
        })
        .await;
        assert!(r.is_err(), "a dead store reported Ok");
        assert_eq!(calls.load(Ordering::SeqCst), 3, "a dead store did not get every attempt");
        // Succeeding at once: one attempt, no retry.
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let r = crate::kernel::Kernel::emit_poll_mark(|| {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })
        .await;
        assert!(r.is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 1, "a healthy store was retried");
    }
}
