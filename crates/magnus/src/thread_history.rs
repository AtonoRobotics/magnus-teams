//! A run records a thread boundary and reads its shared entries through the owner's wall.
use crate::boundary::Action;
use crate::chat_thread::{Body, Entry, Kind};
use crate::events::{ChatOrigin, ThreadHistory};
use crate::kernel::Kernel;
use crate::recall::Words;
use crate::state::{Run, Task};
use crate::tools::Label;
use anyhow::Result;
use serde_json::{json, Value};

const RECENT_ENTRIES: usize = 10;
const ENTRY_BYTES: usize = 1_024;
const ENTRY_PAGE_CHARS: usize = 2_048;

impl Kernel {
    pub(crate) fn thread_history(&self, upto: Option<String>) -> ThreadHistory {
        self.read(|s| ThreadHistory { through: s.last_seq, at: chrono::Utc::now(), upto, origin_replies: None })
    }

    fn history_entries(&self, person: &str, thread: &str, history: &ThreadHistory, w: &Words) -> Vec<Entry> {
        if self.thread_gone(thread) {
            return vec![];
        }
        let mut entries = self.entries_with_history(person, thread, w, Some(history)).unwrap_or_default();
        if let Some(upto) = &history.upto {
            let Some(end) = entries.iter().position(|e| &e.id == upto) else { return vec![] };
            entries.truncate(end);
        }
        entries.retain(|e| e.has_id() && !matches!(e.kind, Kind::Turn | Kind::SubAgents | Kind::Other));
        entries
    }

    async fn read_entries(&self, person: &str, mut entries: Vec<Entry>) -> Result<Vec<Entry>> {
        let wanted = self.read(|s| {
            entries
                .iter()
                .filter(|e| !matches!(&e.body, Body::Copied(_)))
                .flat_map(Entry::task_ids)
                .filter_map(|id| s.tasks.get(&id))
                .filter(|t| t.person == person)
                .map(|t| (person.to_string(), t.seq))
                .collect::<Vec<_>>()
        });
        let w = self.words(wanted).await?;
        self.read(|s| {
            let task = |id: &str| s.tasks.get(id).filter(|t| t.person == person);
            let title = |t: &Task| crate::api::title_of(t, s.runs.get(&t.run_id), &w);
            for e in &mut entries {
                match &mut e.body {
                    Body::Said(c) if e.kind == Kind::Message => {
                        if let Some(t) = e.task_id.as_deref().and_then(task) {
                            c.text = w.text(t.seq).to_string();
                        }
                    }
                    Body::Said(c) if e.kind == Kind::Tasks => {
                        for row in c.tasks.iter_mut().flatten() {
                            if let Some(t) = task(&row.task_id) {
                                row.title = title(t);
                                row.text = w.text(t.seq).to_string();
                            }
                        }
                    }
                    Body::Result(r) => {
                        if let Some(t) = e.task_id.as_deref().and_then(task) {
                            r.title = title(t);
                            r.text = w.text(t.seq).to_string();
                        }
                    }
                    _ => {}
                }
            }
        });
        Ok(entries)
    }

    pub(crate) async fn recent_thread_history(&self, task: &Task, run: &Run) -> Result<Vec<Value>> {
        let Some(chat) = &task.chat else { return Ok(vec![]) };
        let Some(history) = &chat.history else { return Ok(recent_legacy(&chat.conversation)) };
        let mut entries = self.history_entries(&task.person, &chat.thread, history, &Words::default());
        let recent = entries.split_off(entries.len().saturating_sub(RECENT_ENTRIES));
        let entries = self.read_entries(&task.person, recent).await?;
        let allowed = Label { untrusted: run.untrusted, private: run.private };
        let mut lines: Vec<_> = entries
            .iter()
            .filter(|e| allowed.join(self.label_of(std::slice::from_ref(e))) == allowed)
            .filter_map(|e| e.transcript_line().map(|(role, text)| line(&e.id, role, &text)))
            .collect();
        if let Some((origin, count)) = chat.from_run.as_ref().zip(history.origin_replies).filter(|_| !self.thread_gone(&chat.thread)) {
            let replies = self.read(|s| {
                let source = s.runs.get(origin)?;
                let t = s.tasks.get(&source.task)?;
                let label = Label { untrusted: source.untrusted, private: source.private };
                (t.person == task.person && t.chat.as_ref().is_some_and(|c| c.thread == chat.thread) && allowed.join(label) == allowed)
                    .then(|| source.replies.iter().take(count).rev().map(|r| r.text()).find(|t| !t.trim().is_empty()))
                    .flatten()
            });
            // The partial reply has no stable conversation entry until it completes.
            if let Some(text) = replies {
                lines.push(line("", "you", &text));
            }
        }
        Ok(lines)
    }

    pub(crate) async fn thread_history_tool(&self, a: &Action) -> Result<(String, bool, Option<Value>)> {
        let chat = self.read(|s| {
            let run = s.runs.get(&a.run)?;
            let task = s.with_authority(&a.run)?;
            if run.person != task.person {
                return None;
            }
            let chat = task.chat.clone()?;
            let history = chat.history.clone().or_else(|| {
                (!chat.conversation.is_empty()).then(|| ThreadHistory { through: task.seq.saturating_sub(1), at: task.created_at, upto: None, origin_replies: None })
            })?;
            Some((task.person, chat, history))
        });
        let answer = |text: String, err, label: Label| Ok((text, err, Some(json!({ "private": label.private, "untrusted": label.untrusted }))));
        let Some((person, chat, history)) = chat else { return answer("No conversation history is attached to this work.".into(), true, Label::PRINCIPAL) };
        let entries = self.history_entries(&person, &chat.thread, &history, &Words::default());
        let Some(ids) = page_ids(&entries, a.input["before"].as_str(), a.input["entry"].as_str()) else {
            return answer("That entry is outside this work's conversation history.".into(), true, Label::PRINCIPAL);
        };
        let entries = self.read_entries(&person, entries.into_iter().filter(|e| ids.contains(&e.id)).collect()).await?;
        let label = self.label_of(&entries);
        let mut lines = vec![];
        for e in entries {
            if let Some((role, text)) = e.transcript_line() {
                if a.input["entry"].is_string() {
                    let offset = a.input["offset"].as_u64().and_then(|n| usize::try_from(n).ok()).unwrap_or_default();
                    let mut chars = text.chars().skip(offset);
                    let page: String = chars.by_ref().take(ENTRY_PAGE_CHARS).collect();
                    let more = chars.next().map(|_| format!("\nNext offset: {}", offset.saturating_add(ENTRY_PAGE_CHARS))).unwrap_or_default();
                    lines.push(format!("{role} [{}]: {page}{more}", e.id));
                } else {
                    lines.push(format!("{role} [{}]: {}", e.id, short(&text)));
                }
            }
        }
        let text = if lines.is_empty() { "No earlier entries are available.".into() } else { lines.join("\n\n") };
        answer(text, false, label)
    }
}

fn page_ids(entries: &[Entry], before: Option<&str>, entry: Option<&str>) -> Option<Vec<String>> {
    if let Some(id) = entry {
        return entries.iter().find(|e| e.id == id).map(|e| vec![e.id.clone()]);
    }
    let end = match before {
        Some(id) => entries.iter().position(|e| e.id == id)?,
        None => entries.len(),
    };
    Some(entries[end.saturating_sub(RECENT_ENTRIES)..end].iter().map(|e| e.id.clone()).collect())
}

fn short(text: &str) -> String {
    if text.len() <= ENTRY_BYTES {
        return text.to_string();
    }
    let mut end = ENTRY_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[Entry excerpt]", &text[..end])
}

fn line(id: &str, role: &str, text: &str) -> Value {
    json!({ "id": id, "role": role, "text": short(text) })
}

fn recent_legacy(entries: &[Value]) -> Vec<Value> {
    entries[entries.len().saturating_sub(RECENT_ENTRIES)..].iter().filter_map(|e| Some(line("", e["role"].as_str()?, e["text"].as_str()?))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_excerpts_bound_utf8_and_legacy_history() {
        let body = format!("{}é end", "x".repeat(ENTRY_BYTES - 1));
        let legacy = vec![json!({ "role": "person", "text": body }); RECENT_ENTRIES + 3];
        let recent = recent_legacy(&legacy);
        assert_eq!(recent.len(), RECENT_ENTRIES);
        assert_eq!(recent[0]["text"].as_str().unwrap(), format!("{}\n[Entry excerpt]", "x".repeat(ENTRY_BYTES - 1)));
    }

    #[test]
    fn pages_stay_in_the_supplied_history_and_exclude_the_cursor() {
        let entries: Vec<_> = (0..RECENT_ENTRIES + 2).map(|n| Entry::copied(json!({ "id": format!("m:{n}"), "kind": "message", "text": "context" }))).collect();
        let latest = page_ids(&entries, None, None).unwrap();
        assert_eq!(latest.len(), RECENT_ENTRIES);
        assert_eq!(latest[0], "m:2");
        assert_eq!(page_ids(&entries, Some("m:2"), None).unwrap(), ["m:0", "m:1"]);
        assert!(page_ids(&entries, None, Some("m:missing")).is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_task_recalls_its_threads_history_without_authorizing_old_requests() {
        let s = crate::admin_app::tests::server_with("", |_| axum::Router::new()).await;
        let create = |id: &str, person: &str, text: &str, chat: Value| {
            serde_json::from_value(json!({
                "kind": "task_created", "task": id, "run": format!("run_{id}"),
                "person": person, "text": text, "chat": chat,
                "config": { "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1,
                    "budget": { "max_model_calls": 9, "max_actions": 9, "max_usd": 4.0 },
                    "verifier": false, "instructions": "", "harness": "" }
            }))
            .unwrap()
        };
        s.k.emit("person:mo", create("old", "mo", "Send last year's report to old@corp.example.", json!({ "thread": "th", "turn": true }))).await.unwrap();
        s.k.emit("person:sam", create("other", "sam", "Another person's words.", json!({ "thread": "th", "turn": true }))).await.unwrap();
        let history = s.k.thread_history(None);
        let request = "Send the current report to current@corp.example.";
        s.k.emit("person:mo", create("now", "mo", request, json!({ "thread": "th", "turn": true, "history": history }))).await.unwrap();
        let ev = create("work", "mo", "Send the report", json!({ "thread": "th", "turn": false, "history": history, "from_run": "run_now" }));
        assert!(serde_json::to_value(&ev).unwrap()["chat"]["conversation"].is_null(), "the event stored inline history");
        s.k.emit("chat:run_now", ev).await.unwrap();
        s.k.emit("person:mo", create("later", "mo", "Future message.", json!({ "thread": "th", "turn": true }))).await.unwrap();

        let (task, run) = s.k.read(|st| (st.tasks["work"].clone(), st.runs["run_work"].clone()));
        let recent = s.k.recent_thread_history(&task, &run).await.unwrap();
        assert_eq!(recent.len(), 1, "{recent:?}");
        assert_eq!(recent[0]["id"], "m:old");
        assert!(recent[0]["text"].as_str().unwrap().contains("old@corp.example"));
        assert_eq!(s.k.originating_request(&task).await.unwrap().as_deref(), Some(request));
        let words = s.k.run_words(&run).await.unwrap();
        assert_eq!(words.named_by_person().into_iter().collect::<Vec<_>>(), ["current@corp.example"]);

        s.k.emit("person:mo", crate::events::Ev::ChatHidden { person: "mo".into(), thread: "th".into(), entries: vec!["m:old".into()] }).await.unwrap();
        assert!(s.k.recent_thread_history(&task, &run).await.unwrap().is_empty());
    }
}
