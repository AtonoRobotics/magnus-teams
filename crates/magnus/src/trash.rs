//! The trash. Deleting an item moves it here, out of every view; it can be
//! restored until it is removed, `[trash] keep_days` later or when the
//! person empties the trash. Removing deletes the item's own content from
//! the content store; the log keeps the record (C3). Only the item goes:
//! what it made (files a task delivered, memories it wrote, tasks a
//! conversation started) stays, as the owner decided.

use crate::api::{err, internal, who};
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::state::NotTrashable;
use anyhow::Result;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;

type K = State<Arc<Kernel>>;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/trash", get(list).post(put))
        .route("/v1/trash/empty", post(empty))
        .route("/v1/trash/{item}/restore", post(restore))
        .route("/v1/trash/{item}", delete(remove_now))
}

impl Kernel {
    /// Move a person's item to the trash.
    pub async fn trash(&self, person: &str, owner: bool, item: &str) -> Result<Result<(), NotTrashable>> {
        let _one = changing().lock().await;
        if let Err(why) = self.read(|s| s.may_trash(person, owner, item)) {
            return Ok(Err(why));
        }
        self.emit(&format!("person:{person}"), Ev::Trashed { item: item.into(), person: person.into() }).await?;
        if item.starts_with("kb_") {
            self.index_id(item).await?;
        }
        Ok(Ok(()))
    }

    /// Put a trashed item back. False when it is not in the person's trash.
    pub async fn restore(&self, person: &str, item: &str) -> Result<bool> {
        let _one = changing().lock().await;
        if !self.read(|s| s.trash.get(item).is_some_and(|t| t.person == person)) {
            return Ok(false);
        }
        self.emit(&format!("person:{person}"), Ev::Restored { item: item.into() }).await?;
        if item.starts_with("kb_") {
            self.index_id(item).await?;
        }
        Ok(true)
    }

    /// Remove a trashed item: its content leaves the store for good. The
    /// record of which events lost their content is written first, so a
    /// removal cut short finishes at the next start.
    pub async fn remove(&self, actor: &str, item: &str) -> Result<()> {
        self.remove_content(actor, item).await?;
        self.scrub_after_removal().await;
        Ok(())
    }

    /// The removal itself, without the backups.
    async fn remove_content(&self, actor: &str, item: &str) -> Result<()> {
        // One trash change at a time: a restore between reading what the
        // item holds and deleting it would put back an item with no content.
        let _one = changing().lock().await;
        // A memory is found where it was written by what it says, recalled
        // for its person: the kernel does not keep it.
        let memory = self.read(|s| s.trash.get(item).and_then(|t| t.memory().map(|m| (t.person.clone(), m.seq))));
        let fact = match memory {
            Some((person, seq)) => self.recall.content(&person, &[seq]).await?.remove(&seq).and_then(|c| c["fact"].as_str().map(String::from)),
            None => None,
        };
        let Some(holding) = self.read(|s| s.holding(item, fact.as_deref())) else { return Ok(()) };
        let seqs = self.log.content_seqs(&holding).await?;
        self.emit(actor, Ev::Removed { item: item.into(), seqs: seqs.clone() }).await?;
        self.log.delete_content(&seqs).await?;
        crate::ui::images::forget_runs(&self.cfg.appliance.state_dir, &holding.runs);
        Ok(())
    }

    /// Removed content out of the backups kept too. The removal's record
    /// makes a scrub that fails here happen at the next start.
    async fn scrub_after_removal(&self) {
        if let Err(e) = self.scrub_backups().await {
            tracing::error!("taking removed content out of the backups: {e:#}");
        }
    }

    /// Remove what has been in the trash for `keep_days`. One item that
    /// cannot be removed now does not hold back the others; it is tried
    /// again at the next round.
    pub async fn purge_trash(&self) -> Result<usize> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(self.trash_keep_days() as i64);
        let due: Vec<String> = self.read(|s| s.trash.iter().filter(|(_, t)| t.at <= cutoff).map(|(id, _)| id.clone()).collect());
        let mut removed = 0;
        let mut failed = vec![];
        for item in &due {
            match self.remove_content("kernel:trash", item).await {
                Ok(()) => removed += 1,
                Err(e) => failed.push(format!("{item}: {e:#}")),
            }
        }
        if removed > 0 {
            self.scrub_after_removal().await;
        }
        anyhow::ensure!(failed.is_empty(), "{removed} removed; not removed: {}", failed.join("; "));
        Ok(removed)
    }
}

/// Trash changes (trash, restore, remove) one at a time.
fn changing() -> &'static tokio::sync::Mutex<()> {
    static M: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    M.get_or_init(Default::default)
}

/// Remove what is due, hourly from the appliance's start: an item is gone
/// within the hour after its `keep_days`. Backups that still hold removed
/// content are rewritten at start and tried again each hour (#195).
pub fn schedule(k: Arc<Kernel>) {
    tokio::spawn(async move {
        // A removal cut short before its backups were scrubbed, or a scrub
        // that failed before the restart.
        if let Err(e) = k.scrub_backups().await {
            tracing::error!("taking removed content out of the backups: {e:#}");
        }
        let mut t = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            t.tick().await;
            if let Err(e) = k.purge_trash().await {
                tracing::error!("emptying the trash: {e:#}");
            }
            if k.read(|s| s.scrub_pending.is_some()) {
                if let Err(e) = k.scrub_backups().await {
                    tracing::error!("taking removed content out of the backups: {e:#}");
                }
            }
        }
    });
}

macro_rules! person {
    ($k:expr, $h:expr) => {
        match who(&$k, &$h).await {
            Ok(p) => p,
            Err(r) => return r,
        }
    };
}

/// What is in the person's trash, newest first, and when each goes.
async fn list(State(k): K, h: HeaderMap) -> Response {
    let p = person!(k, h);
    let keep = chrono::Duration::days(k.trash_keep_days() as i64);
    // A trashed memory's title is its fact, and a task's, a message's or a
    // conversation's is what the person asked: recalled for the person.
    let seqs: Vec<i64> = k.read(|s| s.trash.values().filter(|t| t.person == p.id).filter_map(|t| t.memory().map(|m| m.seq)).collect());
    let facts = match k.recall.content(&p.id, &seqs).await {
        Ok(c) => c,
        Err(e) => return crate::api::internal(e),
    };
    let asked: Vec<(String, i64)> = k.read(|s| s.trash.values().filter(|t| t.person == p.id).filter_map(|t| t.title_from.map(|f| (p.id.clone(), f.seq()))).collect());
    let words = match k.words(asked).await {
        Ok(w) => w,
        Err(e) => return crate::api::internal(e),
    };
    let mut items: Vec<Value> = k.read(|s| {
        s.trash
            .iter()
            .filter(|(_, t)| t.person == p.id)
            .map(|(id, t)| {
                let title = match (t.memory(), t.title_from) {
                    (Some(m), _) => facts.get(&m.seq).and_then(|c| c["fact"].as_str()).unwrap_or_default().to_string(),
                    (None, Some(from)) => from.title(words.of(from.seq())),
                    (None, None) => t.title.clone(),
                };
                json!({ "item": id, "kind": t.kind(), "title": title.chars().take(120).collect::<String>(), "thread": t.thread(), "trashed_at": t.at, "removed_at": t.at + keep })
            })
            .collect()
    });
    items.sort_by(|a, b| b["trashed_at"].as_str().cmp(&a["trashed_at"].as_str()));
    Json(json!({ "items": items, "keep_days": k.trash_keep_days() })).into_response()
}

/// Move an item to the trash: `{"item": id}`.
async fn put(State(k): K, h: HeaderMap, Json(b): Json<Value>) -> Response {
    let p = person!(k, h);
    let Some(item) = b["item"].as_str() else { return err(StatusCode::BAD_REQUEST, "name the item") };
    trashed(k.trash(&p.id, p.is_owner(), item).await)
}

pub(crate) fn trashed(r: Result<Result<(), NotTrashable>>) -> Response {
    match r {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(NotTrashable::NotFound)) => err(StatusCode::NOT_FOUND, "no such item of yours"),
        Ok(Err(NotTrashable::Running)) => err(StatusCode::CONFLICT, "the task is still running; cancel it first"),
        Err(e) => internal(e),
    }
}

async fn restore(State(k): K, h: HeaderMap, Path(item): Path<String>) -> Response {
    let p = person!(k, h);
    match k.restore(&p.id, &item).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => err(StatusCode::NOT_FOUND, "not in your trash"),
        Err(e) => internal(e),
    }
}

/// Remove one item from the trash now.
async fn remove_now(State(k): K, h: HeaderMap, Path(item): Path<String>) -> Response {
    let p = person!(k, h);
    if !k.read(|s| s.trash.get(&item).is_some_and(|t| t.person == p.id)) {
        return err(StatusCode::NOT_FOUND, "not in your trash");
    }
    match k.remove(&format!("person:{}", p.id), &item).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => internal(e),
    }
}

/// Remove everything in the person's trash now, when space is needed sooner.
async fn empty(State(k): K, h: HeaderMap) -> Response {
    let p = person!(k, h);
    let items: Vec<String> = k.read(|s| s.trash.iter().filter(|(_, t)| t.person == p.id).map(|(id, _)| id.clone()).collect());
    for item in &items {
        if let Err(e) = k.remove_content(&format!("person:{}", p.id), item).await {
            return internal(e);
        }
    }
    k.scrub_after_removal().await;
    Json(json!({ "removed": items.len() })).into_response()
}

impl crate::state::State {
    /// Whether `person` (`owner` or not) may put `item` in the trash now.
    pub fn may_trash(&self, person: &str, owner: bool, item: &str) -> Result<(), NotTrashable> {
        use NotTrashable::*;
        let ended = |run: &str| self.runs.get(run).is_none_or(|r| r.ended_at.is_some());
        let mine = match item.split_once(':') {
            Some(("m" | "a", task)) => {
                let t = self.tasks.get(task).filter(|t| t.person == person && t.is_chat_turn()).ok_or(NotFound)?;
                return if ended(&t.run_id) { Ok(()) } else { Err(Running) };
            }
            _ if item.starts_with("task_") => {
                let t = self.tasks.get(item).filter(|t| t.person == person && !t.is_chat_turn()).ok_or(NotFound)?;
                return if ended(&t.run_id) { Ok(()) } else { Err(Running) };
            }
            _ if item.starts_with("thread_") => {
                let turns: Vec<&crate::state::Task> = self.tasks.values().filter(|t| t.person == person && t.chat.as_ref().is_some_and(|c| c.turn && c.thread == item)).collect();
                if turns.is_empty() && !self.branches.get(item).is_some_and(|b| b.person == person) {
                    return Err(NotFound);
                }
                return if turns.iter().all(|t| ended(&t.run_id)) { Ok(()) } else { Err(Running) };
            }
            _ => match () {
                _ if self.files.get(item).is_some_and(|f| f.person == person) => true,
                _ if self.projects.get(item).is_some_and(|p| p.person == person) => true,
                _ if self.memory.iter().any(|m| m.id == item && m.person == person) => {
                    // Deleting a memory deletes where it was written in its
                    // run, so the run must have ended (invariant 3).
                    let run = self.memory.iter().find(|m| m.id == item).map(|m| m.run.clone()).unwrap_or_default();
                    return if ended(&run) { Ok(()) } else { Err(Running) };
                }
                // The owner moderates what is shared with groups; an entry
                // only some people read is theirs, never the owner's to delete.
                _ => self.knowledge.get(item).is_some_and(|k| {
                    (owner && k.readers.iter().all(|c| c.iter().any(|r| r.starts_with("group:") || *r == format!("person:{person}"))))
                        || k.author == format!("person:{person}")
                        || k.author.strip_prefix("agent:").and_then(|r| self.runs.get(r)).is_some_and(|r| r.person == person)
                }),
            },
        };
        if mine {
            Ok(())
        } else {
            Err(NotFound)
        }
    }
}

#[cfg(test)]
mod others_tests;
