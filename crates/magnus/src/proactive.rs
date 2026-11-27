//! The proactive loop's settings and hours (behavior step 7, #237): each
//! person's working and quiet hours, review cadence and interruption
//! budget, resolved from their preferences over the deployment's config.
//! Anything unparsable falls back to the default; settings never break a
//! review.

mod windows;
use crate::tools::Label;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Arc;
pub use windows::*;

/// A failed poll mark waits this long times its attempt before it is emitted again.
const MARK_RETRY_STEP_MS: u64 = 100;

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
        let people: Vec<String> = self.people().ids();
        for person in people {
            let start = crate::halts::Subject::new_work(&person, false);
            if self.read(|s| crate::halts::may_proceed(s, &start, crate::halts::Phase::Start).is_some() || s.reviewing(&person)) {
                continue;
            }
            let prefs = self.read(|s| s.identity.preferences_of(&person).cloned().unwrap_or(serde_json::Value::Null));
            let st = settings(&self.cfg.proactive, &prefs);
            let m = minutes_at(now, st.tz);
            let today = day_key(now, st.tz);
            let (last, overnight) = self.read(|s| (s.inbox.reviews_last().get(&person).cloned(), s.inbox.reviews_overnight().get(&person).cloned()));
            // One sweep starts at most one review per person: an event
            // first (it is marked, so the cadence cannot starve it, and it
            // is the more urgent), else the periodic one (#381).
            let event: Option<(String, Option<Ended>)> = self.read(|s| {
                for sc in s.arrangements.schedules().values().filter(|sc| sc.person == person && sc.paused.is_none()) {
                    if sc.next_due().is_some_and(|d| d > now && d <= now + chrono::Duration::minutes(st.every_minutes as i64))
                        && !s.inbox.wake_marks().get(&format!("{person}:schedule {}", sc.id)).is_some_and(|d| d == &today)
                    {
                        return Some((schedule_reason(&sc.id), None));
                    }
                }
                if let Some(id) = commitment_to_wake(s, &person, st.tz, now) {
                    return Some((commitment_reason(&id), None));
                }
                for i in s.inbox.items().values().filter(|i| i.person == person && i.kind == "approval" && i.resolution.is_none()) {
                    if now.signed_duration_since(i.created_at).num_minutes() >= 30 && !s.inbox.wake_marks().get(&format!("{person}:approval {}", i.id)).is_some_and(|d| d == &today)
                    {
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
                        if s.inbox.wake_marks().contains_key(&format!("{person}:task {}", t.task_id)) {
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
            } else if in_window(m, st.working) && last.is_none_or(|t| now.signed_duration_since(t).num_minutes() >= st.every_minutes as i64) && !self.state_unchanged(&person, now)
            {
                // Over the same state as the last review, a cadence review
                // has nothing to read that one did not (`review_news`).
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
        let people: Vec<String> = self.people().ids();
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
                let due = self.read(|s| s.watch.last_polled(&person, &app, &w.tool).is_none_or(|t| now.signed_duration_since(t).num_minutes() >= minutes));
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
    pub(crate) async fn review_wake(self: &Arc<Self>, person: &str, reason: String, ended: Option<Ended>) -> bool {
        if !self.cfg.proactive.on {
            return false;
        }
        if self.read(|s| s.reviewing(person) || ended.as_ref().is_some_and(|e| s.inbox.wake_marks().contains_key(&format!("{person}:task {}", e.task)))) {
            return false;
        }
        self.start_review(person, reason, ended).await
    }

    /// Wake one review for new items in a connected app, unless one is
    /// already going (#369). True when a review started: the caller
    /// advances its baseline only then, so a dropped wake retries on
    /// the next read. The reason carries the count only; the review
    /// pulls the items itself, tainting normally through the boundary.
    /// Items gather for a while after the last such review, unless they
    /// are `urgent` (`wake_budget::gathering`).
    pub async fn review_wake_items(self: &Arc<Self>, person: &str, app: &str, n: usize, urgent: bool) -> bool {
        if !self.cfg.proactive.on {
            return false;
        }
        let gather = self.cfg.proactive.arrival_gather_minutes.unwrap_or(crate::wake_budget::ARRIVAL_GATHER_MINUTES);
        if self.read(|s| s.reviewing(person) || (!urgent && crate::wake_budget::gathering(s, person, Utc::now(), gather))) {
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
            let window = self.read(|s| s.watch.window(person, app, remote).cloned());
            let Some(window) = window else {
                let _ = self
                    .emit(
                        &format!("person:{person}"),
                        crate::events::Ev::WatchedSeen(crate::events::WatchedSeenPayload { person: person.into(), app: app.into(), tool: remote.into(), ids, full: true }),
                    )
                    .await;
                continue;
            };
            let fresh: Vec<&String> = ids.iter().filter(|id| !window.iter().any(|w| w.contains(*id))).collect();
            if fresh.is_empty() {
                // No news, but the read still counts: the window is the
                // last two reads, whatever they held (#414).
                let _ = self
                    .emit(
                        &format!("person:{person}"),
                        crate::events::Ev::WatchedSeen(crate::events::WatchedSeenPayload { person: person.into(), app: app.into(), tool: remote.into(), ids, full: true }),
                    )
                    .await;
                continue;
            }
            let n = fresh.len();
            let triage = crate::ambient_watch::triage_watched_diff(app, remote, n, data);
            if !triage.actionable || triage.urgency == crate::ambient_watch::Urgency::Ignore {
                let _ = self
                    .emit(
                        &format!("person:{person}"),
                        crate::events::Ev::WatchedSeen(crate::events::WatchedSeenPayload { person: person.into(), app: app.into(), tool: remote.into(), ids, full: true }),
                    )
                    .await;
                continue;
            }
            if self.review_wake_items(person, app, n, triage.urgency == crate::ambient_watch::Urgency::High).await {
                let _ = self
                    .emit(
                        &format!("person:{person}"),
                        crate::events::Ev::WatchedSeen(crate::events::WatchedSeenPayload { person: person.into(), app: app.into(), tool: remote.into(), ids, full: true }),
                    )
                    .await;
            }
        }
    }

    /// Start the review run itself: the review instruction over the
    /// person's state, on the per-task budget, which its Auto Review calls
    /// are charged to as well (#237, spec rev 133).
    /// Every review starts here, so every one counts against the person's
    /// wake budget (`wake_budget`), and none starts past it.
    async fn start_review(self: &Arc<Self>, person: &str, reason: String, ended: Option<Ended>) -> bool {
        if let Some(why) = self.read(|s| crate::wake_budget::refused(s, person, Utc::now())) {
            tracing::debug!("no review for {person} ({reason}): {why}");
            return false;
        }
        self.note_reviewed_state(person, Utc::now());
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
        let (tasks, schedules, approvals, disagreements, commitment_meta) = self.read(|s| {
            let tasks = s
                .tasks
                .values()
                .filter(|t| t.person == person && !t.review && !t.poll && s.runs.get(&t.run_id).is_some_and(|r| !r.status.terminal()))
                .map(|t| (t.person.clone(), t.seq))
                .collect::<Vec<_>>();
            let schedules = s
                .arrangements
                .schedules()
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
            let approvals =
                s.inbox.items().values().filter(|i| i.person == person && i.kind == "approval" && i.resolution.is_none()).map(|i| i.summary.clone()).collect::<Vec<_>>();
            let disagreements = s
                .thread_extras
                .disagreements()
                .values()
                .filter(|d| d.person == person && d.outcome.is_none())
                .map(|d| format!("{} -> {}", d.position.chars().take(120).collect::<String>(), d.decision))
                .collect::<Vec<_>>();
            let commitment_meta = s
                .arrangements
                .commitments()
                .values()
                .filter(|c| c.person == person && c.status == "open")
                .map(|c| {
                    let zone = s.identity.zone(person);
                    let overdue = c.by.as_deref().and_then(|b| crate::commitments::due(b, zone)).is_some_and(|d| d < Utc::now());
                    (c.id.clone(), c.seq, c.by.clone(), overdue)
                })
                .collect::<Vec<_>>();
            (tasks, schedules, approvals, disagreements, commitment_meta)
        });
        let c_seqs: Vec<i64> = commitment_meta.iter().map(|(_, seq, ..)| *seq).collect();
        let c_content = if c_seqs.is_empty() { std::collections::HashMap::new() } else { self.recall.content(person, &c_seqs).await.unwrap_or_default() };
        let commitments: Vec<String> = commitment_meta
            .into_iter()
            .map(|(id, seq, by, overdue)| {
                let what = c_content.get(&seq).and_then(|v| v["what"].as_str()).unwrap_or_default();
                let to = c_content.get(&seq).and_then(|v| v["to"].as_str()).unwrap_or_default();
                let detail = match (what.is_empty(), to.is_empty()) {
                    (false, false) => format!(": {what} (to {to})"),
                    (false, true) => format!(": {what}"),
                    _ => String::new(),
                };
                format!("{id}{detail} (by {}){}", by.as_deref().unwrap_or("no date"), if overdue { ", OVERDUE" } else { "" })
            })
            .collect();
        let told = self.read(|s| crate::review_news::told_lines(s, person, Utc::now()));
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
            "{woke}{}{}{}{}{}{}",
            section("Unfinished work", open),
            section("Schedules", schedules),
            section("Approvals waiting", approvals),
            section("Open disagreements", disagreements),
            section("Open commitments", commitments),
            section("Already told them (do not tell again what these say)", told)
        )
        .chars()
        .take(6000)
        .collect()
    }

    /// Now, or when quiet ends while it is quiet: when an observation or
    /// interruption may ask for attention (#237).
    pub fn notify_due(&self, person: &str) -> DateTime<Utc> {
        let prefs = self.read(|s| s.identity.preferences_of(person).cloned().unwrap_or(serde_json::Value::Null));
        let st = settings(&self.cfg.proactive, &prefs);
        let now = Utc::now();
        quiet_ends_at(now, st.tz, st.quiet).unwrap_or(now)
    }

    /// Deliver an interruption, or queue it as an observation: outside
    /// working hours or over budget it waits for the next session; in
    /// quiet hours it posts now and pings when quiet ends (#237). What
    /// the `interrupt` record says back.
    /// Whether the loop may `op` for this run (`State::loop_deny`).
    pub(crate) fn loop_deny(&self, run: &crate::state::Run, op: &str, tool: Option<&crate::policy::ToolFacts>) -> Option<String> {
        let person = self.person(&run.person);
        self.read(|s| s.loop_deny(run, person.as_ref(), op, tool))
    }

    pub async fn interrupt_for(&self, run: &crate::state::Run, what: &str, why: &str, call_id: &str) -> anyhow::Result<(String, bool)> {
        let prefs = self.read(|s| s.identity.preferences_of(&run.person).cloned().unwrap_or(serde_json::Value::Null));
        let st = settings(&self.cfg.proactive, &prefs);
        let now = Utc::now();
        if let Some(told) = self.already_told(&run.person, what, why, "") {
            return Ok((told, false));
        }
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
            return Ok((format!("Outside working hours: queued as observation {item}; it pings at {}.", st.tz.local(at).format("%H:%M")), false));
        }
        let today = day_key(now, st.tz);
        let n = self.read(|s| s.inbox.interrupts().get(&run.person).filter(|(d, _)| *d == today).map(|(_, n)| *n).unwrap_or(0));
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
            Ok((format!("Posted as interruption {item}; it pings when quiet ends at {}.", st.tz.local(at).format("%H:%M")), false))
        } else {
            Ok((format!("Delivered as interruption {item}."), false))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cadence::Zone;

    /// Watched ids come from the pointed array's id field; misshapen
    /// answers and id-less items yield nothing, never a wake (#369).
    /// Windows hold across midnight; garbage falls back to defaults.
    #[test]
    fn hours_windows_and_fallbacks() {
        assert!(in_window(600, (540, 1020)) && !in_window(1020, (540, 1020)));
        assert!(in_window(1400, (1320, 420)) && in_window(100, (1320, 420)) && !in_window(500, (1320, 420)));
        let cfg = crate::config::Proactive::default();
        let s = settings(&cfg, &serde_json::json!({ "working_hours": { "start": "xx", "end": "17:00" }, "timezone": "Mars/Olympus", "review_every_minutes": 0 }));
        assert_eq!((s.working, s.tz, s.every_minutes), ((540, 1020), Zone::Server, 60));
        assert_eq!(day_key(DateTime::parse_from_rfc3339("2026-11-07T06:30:00Z").unwrap().into(), Zone::Named(chrono_tz::America::Los_Angeles)), "2026-11-06");
        let ends = quiet_ends_at(DateTime::parse_from_rfc3339("2026-11-07T06:30:00Z").unwrap().into(), Zone::Named(chrono_tz::Tz::UTC), (1320, 420)).unwrap();
        assert_eq!(ends.to_rfc3339(), "2026-11-07T07:00:00+00:00");
        assert!(quiet_ends_at(DateTime::parse_from_rfc3339("2026-11-07T12:00:00Z").unwrap().into(), Zone::Named(chrono_tz::Tz::UTC), (1320, 420)).is_none());
        let work = (540, 1020);
        assert!(window_next_start(DateTime::parse_from_rfc3339("2026-11-07T12:00:00Z").unwrap().into(), Zone::Named(chrono_tz::Tz::UTC), work).is_none());
        assert_eq!(
            window_next_start(DateTime::parse_from_rfc3339("2026-11-07T08:00:00Z").unwrap().into(), Zone::Named(chrono_tz::Tz::UTC), work).unwrap().to_rfc3339(),
            "2026-11-07T09:00:00+00:00"
        );
        assert_eq!(
            window_next_start(DateTime::parse_from_rfc3339("2026-11-07T18:00:00Z").unwrap().into(), Zone::Named(chrono_tz::Tz::UTC), work).unwrap().to_rfc3339(),
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

    #[test]
    fn commitment_due_for_wake_includes_overdue() {
        let now: DateTime<Utc> = DateTime::parse_from_rfc3339("2026-11-24T12:00:00Z").unwrap().into();
        let overdue = now - chrono::Duration::hours(3);
        let due_soon = now + chrono::Duration::hours(6);
        let due_later = now + chrono::Duration::hours(30);
        assert!(commitment_due_for_wake(overdue, now), "overdue commitment must wake");
        assert!(commitment_due_for_wake(due_soon, now), "commitment within 24h must wake");
        assert!(!commitment_due_for_wake(due_later, now), "commitment > 24h away should not wake yet");
    }

    /// One review for each time a commitment is due: a day before it that
    /// crosses the person's midnight wakes one review, not one on each side.
    #[test]
    fn a_commitment_wakes_one_review_for_each_time_it_is_due() {
        use crate::log::Event;
        use serde_json::json;
        let at = |t: &str| DateTime::parse_from_rfc3339(t).unwrap().to_utc();
        let ev = |seq, t: &str, v: Value| Event { seq, at: at(t), actor: "kernel".into(), ev: serde_json::from_value(v).unwrap() };
        let utc = Zone::Named(chrono_tz::Tz::UTC);
        let mut s = crate::state::State::default();
        s.apply(&ev(
            1,
            "2026-11-24T08:00:00Z",
            json!({ "kind": "commitment_recorded", "id": "cm1", "person": "alice", "who": "Dana", "what": "the deck", "by": "2026-11-25T09:00:00Z" }),
        ));
        assert_eq!(commitment_to_wake(&s, "alice", utc, at("2026-11-24T22:00:00Z")).as_deref(), Some("cm1"), "a commitment due within the day woke nothing");
        s.apply(&ev(2, "2026-11-24T22:00:00Z", json!({ "kind": "review_woken", "run": "r1", "task": "t1", "person": "alice", "reason": commitment_reason("cm1") })));
        assert_eq!(commitment_to_wake(&s, "alice", utc, at("2026-11-25T01:00:00Z")), None, "the commitment woke a second review after the person's midnight");
        // Due later now: that time wakes a review of its own.
        s.apply(&ev(3, "2026-11-25T02:00:00Z", json!({ "kind": "commitment_changed", "id": "cm1", "person": "alice", "by": "2026-11-25T20:00:00Z" })));
        assert_eq!(commitment_to_wake(&s, "alice", utc, at("2026-11-25T03:00:00Z")).as_deref(), Some("cm1"), "a new due time woke nothing");
    }
}
