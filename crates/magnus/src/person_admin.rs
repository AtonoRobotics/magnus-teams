//! What root does about one person (admin console spec, Users: Schedules and
//! triggers, Data, Delete), as the kernel's own operations: the console
//! reaches them over the control socket, the app's owner routes call the
//! same ones.
//!
//! - A person's schedules, each suspended or resumed by an admin.
//! - A person's data read: their tasks, conversations, files and memory,
//!   recalled as them, through their wall, by that person alone. A Magnus
//!   admin supports accounts and the server and reads no one's data (the
//!   owner, 2026-11-11, replacing ruling 14). There is no support session
//!   a person grants yet, so there is no way in for anyone else.
//! - A person's data deleted: their runs stopped and schedules ended, their
//!   computer and its volume deleted, their sessions and their own sign-ins
//!   to apps revoked, their team messages deleted for everyone, their
//!   content out of the store and the backups, and their records out of
//!   the appliance's state. What is left of them is the log's record that they existed.

use crate::config::Person;
use crate::events::Ev;
use crate::kernel::Kernel;
use axum::http::StatusCode;
use serde_json::{json, Value};

pub type Refusal = (StatusCode, String);

fn failed(e: anyhow::Error) -> Refusal {
    tracing::error!("person admin: {e:#}");
    (StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
}

pub const KINDS: [&str; 4] = ["tasks", "chats", "files", "memory"];

/// Why someone else's data read is refused.
pub const NOT_THE_ADMINS: &str = "A person's data is theirs: a Magnus admin supports accounts and the server, and reads no one's data.";

/// How much of a file's text a read shows.
const FILE_CHARS: usize = 20_000;

impl Kernel {
    /// A person's schedules, as an admin sees them: its rule, its runs, its
    /// failures, whether it is paused and why. Not what it does: its words
    /// are the person's data, which an admin reads only through
    /// [`Kernel::read_person_data`], recorded and told (spec rev 141,
    /// People: every admin read of a person's data is logged).
    pub async fn person_schedules(&self, person: &str) -> Result<Vec<Value>, Refusal> {
        Ok(self.read(|s| {
            let mut v: Vec<Value> = s
                .schedules
                .values()
                .filter(|x| x.person == person)
                .map(|x| {
                    let runs = s.tasks.values().filter(|t| t.schedule.as_deref() == Some(x.id.as_str())).count();
                    json!({
                        "id": x.id,
                        "every_minutes": x.every_minutes, "daily_at": x.daily_at, "next": x.next_due(), "last_fired": x.last_fired,
                        "until": x.until, "max_runs": x.max_runs, "runs": runs, "failures": x.failures.len(),
                        "skipped": x.skipped.as_ref().map(|(at, _, why)| json!({ "at": at, "why": why })),
                        "paused": x.paused, "paused_on": x.paused_on, "ended": x.ended,
                    })
                })
                .collect();
            v.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
            v
        }))
    }

    /// Suspend or resume one of a person's schedules, as an admin. A
    /// schedule whose run waits on its person resumes when they answer it.
    pub async fn admin_schedule(&self, actor: &str, by: &str, person: &str, id: &str, on: bool) -> Result<Value, Refusal> {
        let Some(sc) = self.read(|s| s.schedules.get(id).filter(|x| x.person == person).cloned()) else { return Err((StatusCode::NOT_FOUND, "no such schedule".into())) };
        if on {
            if sc.paused.is_none() {
                return Err((StatusCode::CONFLICT, "the schedule is not suspended".into()));
            }
            if sc.paused_on.is_some() {
                return Err((StatusCode::CONFLICT, "its run is waiting on its person: when they answer it, the schedule goes on".into()));
            }
            self.emit(actor, Ev::ScheduleResumed { id: id.into(), source: Some("console".into()), turn: None }).await.map_err(failed)?;
        } else {
            if sc.ended.is_some() {
                return Err((StatusCode::CONFLICT, "the schedule has ended".into()));
            }
            if sc.paused.is_some() {
                return Err((StatusCode::CONFLICT, "the schedule is already suspended".into()));
            }
            self.emit(
                actor,
                Ev::SchedulePaused { id: id.into(), reason: format!("Suspended by {by}, an admin"), task: None, admin: true, source: Some("console".into()), turn: None },
            )
            .await
            .map_err(failed)?;
        }
        Ok(json!({ "id": id, "paused": !on }))
    }

    /// A person's data, or one item of it, read by `reader` (`by` is their
    /// name). Refused when the organisation's policy keeps people's data
    /// private. A read of someone else's is recorded, and they are told.
    pub async fn read_person_data(&self, reader: &str, by: &str, target: &Person, what: &str, item: Option<&str>) -> Result<Value, Refusal> {
        if !KINDS.contains(&what) {
            return Err((StatusCode::NOT_FOUND, "tasks, chats, files or memory".into()));
        }
        let own = reader == format!("person:{}", target.id);
        // A person's data is theirs: a Magnus admin supports their account
        // and the server and reads none of it (the owner, 2026-11-11).
        if !own {
            return Err((StatusCode::FORBIDDEN, NOT_THE_ADMINS.into()));
        }
        // Their tasks' words are recalled as them, through their wall.
        let words = match what {
            "tasks" | "files" | "chats" => self.person_words(&target.id).await.map_err(failed)?,
            _ => Default::default(),
        };
        let none = || (StatusCode::NOT_FOUND, format!("no such item in their {what}"));
        let v: Value = match (what, item) {
            ("tasks", None) => json!(self.read(|s| {
                s.tasks
                    .values()
                    .filter(|t| t.person == target.id && !t.is_chat_turn())
                    .filter_map(|t| s.runs.get(&t.run_id).map(|r| crate::api::task_row(self, s, target, t, r, &words)))
                    .collect::<Vec<Value>>()
            })),
            ("tasks", Some(id)) => {
                // A chat turn is not a task: the list leaves turns out, and
                // so does this, or a turn's tool calls (among them the
                // person's relationship notes) come back here (#257).
                let found = self.read(|s| {
                    s.tasks
                        .get(id)
                        .filter(|t| t.person == target.id && !t.is_chat_turn())
                        .and_then(|t| s.runs.get(&t.run_id).map(|r| (crate::api::task_row(self, s, target, t, r, &words), r.clone())))
                });
                let (row, run) = found.ok_or_else(none)?;
                json!({ "task": row, "timeline": crate::api::timeline_of(&run, !own) })
            }
            ("chats", None) => json!(self.threads(&target.id).await.map_err(failed)?),
            ("chats", Some(id)) => json!({ "thread": id, "entries": self.thread_entries(&target.id, id).await.map_err(failed)?.ok_or_else(none)? }),
            ("files", None) => json!(self.read(|s| crate::library::files_matching(s, target, &[], &Default::default(), None, &words))),
            ("files", Some(id)) => {
                let f = self.read(|s| s.files.get(id).filter(|f| f.person == target.id).cloned()).ok_or_else(none)?;
                let info = self.read(|s| crate::library::file_json(s, &f, &words));
                let bytes = self.file_bytes(&f).await.map_err(failed)?.unwrap_or_default();
                let text = (f.media_type.starts_with("text/") || ["application/json", "application/xml", "application/csv"].contains(&f.media_type.as_str()))
                    .then(|| String::from_utf8_lossy(&bytes).chars().take(FILE_CHARS).collect::<String>());
                json!({ "file": info, "bytes": bytes.len(), "text": text })
            }
            ("memory", item) => {
                let all = self.remembered(&target.id, |_| true).await.map_err(failed)?;
                let rows: Vec<Value> = all
                    .into_iter()
                    .filter(|r| item.is_none_or(|i| r.entry.id == i))
                    .map(|r| json!({ "id": r.entry.id, "fact": r.fact, "topic": r.topic, "at": r.entry.at }))
                    .collect();
                match item {
                    Some(_) => rows.into_iter().next().ok_or_else(none)?,
                    None => json!(rows),
                }
            }
            _ => return Err(none()),
        };
        if !own {
            self.record_data_read(reader, by, &[target.id.clone()], what, item).await.map_err(failed)?;
        }
        Ok(json!({ "person": target.id, what: v }))
    }

    /// `reader` (`by` is their name) read these people's data: each read is
    /// in the log, and each person told (owner's ruling 14).
    pub async fn record_data_read(&self, reader: &str, by: &str, people: &[String], what: &str, item: Option<&str>) -> anyhow::Result<()> {
        let said = match item {
            None => format!("{by} read your {what}"),
            Some(_) => format!("{by} opened one of your {what}"),
        };
        let mut evs = vec![];
        for person in people {
            evs.push((reader, Ev::PersonDataRead { person: person.clone(), what: what.to_string(), item: item.map(String::from) }));
            evs.push((
                "kernel",
                self.notice_to(
                    None,
                    None,
                    person.clone(),
                    said.clone(),
                    json!({ "reason": "data_read", "evidence": { "by": reader.strip_prefix("person:").unwrap_or(reader), "what": what, "item": item } }),
                ),
            ));
        }
        self.emit_all(evs).await?;
        Ok(())
    }

    /// Delete a person's data, as `actor`. Their runs stop, their schedules
    /// end, their computer is deleted, their sessions and their own sign-ins to apps are revoked, their
    /// content leaves the store and the backups, and their records leave
    /// the appliance's state. What it did, counted.
    pub async fn erase_person(&self, actor: &str, person: &str) -> Result<Value, Refusal> {
        // Runs still going stop first, so nothing writes for them after.
        let going: Vec<String> = self.read(|s| s.runs.values().filter(|r| r.person == person && !r.status.terminal()).map(|r| r.id.clone()).collect());
        for run in &going {
            if self.emit(actor, Ev::Cancelled { run: run.clone(), source: Some("console".into()), turn: None }).await.is_ok() {
                self.on_stop(run, true).await;
            }
        }
        let schedules: Vec<String> = self.read(|s| s.schedules.values().filter(|x| x.person == person).map(|x| x.id.clone()).collect());
        for id in &schedules {
            self.emit(actor, Ev::ScheduleRemoved { id: id.clone(), source: Some("console".into()), turn: None }).await.map_err(failed)?;
        }
        // Their brokered connectors' microVMs.
        self.brokered.stop(None, Some(person)).await;
        // Their computer and its volume, a config person's too (owner,
        // 2026-11-13). Nothing after this runs until it is gone.
        let computers = self.delete_computers_of(person).await.map_err(|e| {
            tracing::error!("erasing {person}: their computer could not be deleted: {e:?}");
            (StatusCode::BAD_GATEWAY, "their computer could not be deleted, so the erasure stopped there; try again".to_string())
        })?;
        // Every device signed in as them.
        let ended = self.accounts.end_all(person).await.map_err(failed)?;
        let sessions = ended.len();
        let evs: Vec<(&str, Ev)> = ended.into_iter().map(|session| (actor, Ev::SignedOut { person: person.to_string(), session })).collect();
        if !evs.is_empty() {
            self.emit_all(evs).await.map_err(failed)?;
        }
        // Their own sign-ins to the organisation's apps, revoked at each app.
        let mine: Vec<String> = self.vault.names().map_err(failed)?.into_iter().filter(|n| crate::forget::their_sign_in(n, person)).collect();
        let mut sign_ins = 0;
        for name in mine {
            if let Some(s) = self.vault.remove(&name).map_err(failed)? {
                let (done, detail) = crate::connectors::revoke(self, s.expose()).await;
                tracing::info!("erasing {person}: revoked {name} {done}: {detail}");
                sign_ins += 1;
            }
        }
        // Their card, their Link wallet and the tokens of apps they built.
        let built: Vec<String> = self.read(|s| s.connectors.values().filter(|c| c.owner.as_deref() == Some(person)).filter_map(|c| c.credential.clone()).collect());
        for name in [format!("payments/card/{person}"), format!("payments/link/{person}")].into_iter().chain(built) {
            self.vault.remove(&name).map_err(failed)?;
        }
        // Their content: recorded as removed first (a removal cut short
        // finishes at the next start), then deleted, then out of the backups.
        // Their team messages are deleted for everyone first, as a sender's
        // own delete is, so the channels keep their places (owner, 2026-11-13).
        let said = self.unsend_all_of(actor, person).await.map_err(failed)?;
        let mut seqs = self.log.person_content_seqs(person).await.map_err(failed)?;
        seqs.extend(said);
        seqs.sort_unstable();
        seqs.dedup();
        let n = seqs.len();
        if !seqs.is_empty() {
            self.emit(actor, Ev::Removed { item: format!("person:{person}"), seqs: seqs.clone() }).await.map_err(failed)?;
            self.log.delete_content(&seqs).await.map_err(failed)?;
        }
        let runs: Vec<String> = self.read(|s| s.runs.values().filter(|r| r.person == person).map(|r| r.id.clone()).collect());
        crate::ui::images::forget_runs(&self.cfg.appliance.state_dir, &runs);
        self.emit(actor, Ev::PersonErased { person: person.to_string() }).await.map_err(failed)?;
        // Not complete while a backup still holds their content: the backups
        // left are recorded and tried again until they are rewritten (#195).
        let mut out = json!({ "runs_stopped": going.len(), "schedules_ended": schedules.len(), "sessions_ended": sessions, "sign_ins_revoked": sign_ins, "computers_deleted": computers, "content_removed": n, "erasure": "complete" });
        if n > 0 {
            if let Err(e) = self.scrub_backups().await {
                tracing::error!("erasing {person}: their content is still in backups: {e:#}");
                out["erasure"] = json!("incomplete");
                out["backups_pending"] = json!(self.read(|s| s.scrub_pending.as_ref().map(|p| p.files.clone())).unwrap_or_default());
                out["detail"] = json!(format!("their content is still in backups, which are rewritten without it at the next try (hourly, and at start): {e:#}"));
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    /// Erased, a person's card, wallet and own apps' tokens leave the vault
    /// with them, so nothing of theirs is left for anyone to use.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_erasure_takes_a_persons_card_wallet_and_app_tokens() {
        let s = crate::admin_app::tests::server_with("", |_| axum::Router::new()).await;
        let mut own = crate::catalog::connector(&crate::catalog::manifests()[0]);
        own.name = "mo_tool_1a2b3c4d".into();
        own.owner = Some("mo".into());
        own.credential = Some("connector_mo_tool_1a2b3c4d".into());
        s.k.emit("person:mo", crate::events::Ev::ConnectorAdded { connector: own }).await.unwrap();
        let theirs = ["payments/card/mo", "payments/link/mo", "connector_mo_tool_1a2b3c4d"];
        for n in theirs {
            s.k.vault.put(n, "mo's").unwrap();
        }
        s.k.vault.put("payments/card/sam", "sam's").unwrap();
        s.k.erase_person("person:sam", "mo").await.unwrap();
        for n in theirs {
            assert!(s.k.vault.get(n).unwrap().is_none(), "{n} outlived mo's erasure");
        }
        assert!(s.k.vault.get("payments/card/sam").unwrap().is_some(), "someone else's card went with mo");
        s.done().await;
    }
}
