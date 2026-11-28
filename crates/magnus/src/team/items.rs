//! The team chat's items as they travel to the app: a person's message, the
//! agent's live work, and its answer. They are the personal chat's entries
//! (`crate::chat_entry::ChatEntry`), one type for both chats
//! (decisions/2026-11-13-one-chat-entry.md): a message or answer is the
//! personal chat's `message` or `answer`, work no message carries is a
//! `tasks` entry with its one row, and a request's work rides on its message
//! as that row. A deleted message's place is `deleted`. The app's JSDoc
//! (`TeamMessage` in desktop/ui/team-chat.js) follows them.
use super::member_view::{author, Answer, MemberView, Runs, Work};
use super::{Message, Workspace};
use crate::chat_entry::{ChatEntry, EntryFile, TaskRow};
use crate::state::Status;
use std::collections::BTreeMap;

type At = chrono::DateTime<chrono::Utc>;

/// A team channel's item: the one chat entry.
pub type TeamItem = ChatEntry;

/// A request's work as the personal chat lists a task a message started,
/// with whom it is for, how far it is and where it shows.
pub fn task_row(w: &Work) -> TaskRow {
    TaskRow {
        task_id: w.task.clone(),
        title: w.title.clone(),
        text: w.title.clone(),
        status: w.status,
        waiting_on: w.reason.clone(),
        waiting_item: None,
        // What it is doing, in the words a personal task's card uses.
        status_line: w.doing.as_deref().filter(|_| w.status == Status::Running).map(crate::tool_registry::doing_words),
        requester: Some(w.requester.clone()),
        steps: Some(w.steps),
        doing: w.doing.clone(),
        long: Some(w.long),
        thread: w.thread.clone(),
        started: Some(w.started),
    }
}

/// The files a message carries: its lines that begin with a paperclip.
pub fn attached(text: &str) -> Vec<String> {
    text.lines().filter_map(|l| l.strip_prefix("📎 ")).map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect()
}

fn bare(kind: &'static str, id: String, sender: String, requester: Option<String>, text: String, thread: Option<String>, at: At) -> TeamItem {
    let files: Vec<EntryFile> = attached(&text).iter().map(|n| EntryFile::named(n)).collect();
    ChatEntry { id, kind, files: (!files.is_empty()).then_some(files), sender: Some(sender), requester, text, thread, at: Some(at), ..Default::default() }
}

/// A message, with its edits, pin, reactions and whether it was deleted.
pub fn message(w: &Workspace, m: &Message) -> TeamItem {
    let (sender, requester) = author(&m.sender);
    let deleted = w.chat.unsent.contains(&m.id);
    let kind = if deleted {
        "deleted"
    } else if crate::agent_id::is_agent(sender) {
        "answer"
    } else {
        "message"
    };
    ChatEntry {
        edited: Some(w.chat.edited.contains(&m.id)),
        pinned: Some(w.chat.pins.contains(&m.id)),
        reactions: Some(w.chat.reactions.get(&m.id).cloned().unwrap_or_default()),
        deleted: Some(deleted),
        ..bare(kind, m.id.clone(), sender.to_string(), requester.map(String::from), m.text.clone(), m.thread.clone(), m.at)
    }
}

/// A request's work that no message carries, one the agent began itself:
/// the personal chat's `tasks` entry, with its one row.
pub fn work(w: &Work) -> TeamItem {
    ChatEntry {
        from_run: Some(w.task.clone()),
        tasks: Some(vec![task_row(w)]),
        ..bare("tasks", format!("work:{}", w.task), crate::agent_id::of(&w.requester), Some(w.requester.clone()), String::new(), w.asked_in.clone(), w.started)
    }
}

/// The agent's answer, as the viewer may read it.
pub fn answer(a: &Answer) -> TeamItem {
    ChatEntry {
        task: Some(a.task.clone()),
        task_id: Some(a.task.clone()),
        private: Some(a.private),
        ..bare("answer", a.id.clone(), crate::agent_id::of(&a.requester), Some(a.requester.clone()), a.text.clone(), a.thread.clone(), a.at)
    }
}

/// A channel as this person reads it now, in order: its messages with the
/// agent's work cards on the ones that asked, work no message carries, and
/// the agent's answers; each with how many replies hang from it. A deleted
/// message is left only where replies still hang from it.
pub fn timeline<R: Runs>(v: &MemberView<R>, channel: &str, now: At) -> Vec<TeamItem> {
    let w = v.workspace();
    // As a personal chat turn gives way to its answer, a short request's
    // card goes once it has answered; long work keeps its row, as a task a
    // personal message started keeps its card.
    let mut work = v.work(channel, now);
    work.retain(|x| x.long || !matches!(x.status, Status::Completed | Status::Failed));
    let mut items: Vec<TeamItem> =
        v.places(channel).into_iter().map(|m| ChatEntry { work: work.iter().find(|x| x.asked.as_deref() == Some(m.id.as_str())).map(task_row), ..message(w, m) }).collect();
    items.extend(work.iter().filter(|x| x.asked.is_none()).map(self::work));
    items.extend(v.answers(channel, now).iter().map(answer));
    items.sort_by_key(|i| i.at);
    let mut replies: BTreeMap<String, (usize, Option<At>)> = BTreeMap::new();
    for i in &items {
        if let Some(t) = &i.thread {
            let e = replies.entry(t.clone()).or_insert((0, i.at));
            e.0 += 1;
            e.1 = i.at;
        }
    }
    for i in &mut items {
        if let Some((n, last)) = replies.get(&i.id) {
            (i.replies, i.last_reply) = (Some(*n), *last);
        }
    }
    items.retain(|i| i.deleted != Some(true) || i.replies.is_some());
    for i in &mut items {
        i.may = super::may::may(v, channel, i);
    }
    items
}

/// A message's words without its files' lines: the files travel as `files`.
pub fn words(text: &str) -> String {
    text.split('\n').filter(|l| !l.starts_with("📎 ")).collect::<Vec<_>>().join("\n")
}

/// Items as they leave for the app: the words without the files' lines, and
/// whom the agent's answer or work is for by name (`for`). The app drew both
/// itself before; an older one finds the same fields it read then.
pub fn for_the_app(items: Vec<TeamItem>, names: &BTreeMap<String, String>) -> Vec<TeamItem> {
    items
        .into_iter()
        .map(|i| {
            let for_name = i.requester.as_ref().filter(|_| matches!(i.kind, "answer" | "tasks")).map(|r| names.get(r).cloned().unwrap_or_else(|| r.clone()));
            ChatEntry { text: words(&i.text), for_name, ..i }
        })
        .collect()
}

/// What was said in a channel, for this person: its messages and the
/// agent's answers, in order, nothing deleted.
pub fn said<R: Runs>(v: &MemberView<R>, channel: &str, now: At) -> Vec<TeamItem> {
    let w = v.workspace();
    let mut items: Vec<TeamItem> = v.messages(channel).into_iter().map(|m| message(w, m)).collect();
    items.extend(v.answers(channel, now).iter().map(answer));
    items.sort_by_key(|i| i.at);
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at() -> At {
        chrono::DateTime::from_timestamp(0, 0).unwrap_or_default()
    }

    /// Each item says which of the chat view's kinds it is, and its files,
    /// so the app reads it as a personal chat entry.
    #[test]
    fn an_item_is_one_of_the_chat_views_kinds_with_its_files() {
        let said = bare("message", "m1".into(), "sam".into(), None, "see this\n📎 plan.pdf\n📎  \n📎 shot.png".into(), None, at());
        let v = serde_json::to_value(&said).unwrap_or_default();
        assert_eq!(v["kind"], "message");
        assert_eq!(v["files"], serde_json::json!([{ "id": "plan.pdf", "name": "plan.pdf" }, { "id": "shot.png", "name": "shot.png" }]), "a team file is not a chat entry's file");
        let quiet = serde_json::to_value(bare("answer", "a1".into(), crate::agent_id::of("mo"), None, "done".into(), None, at())).unwrap_or_default();
        assert_eq!(quiet["kind"], "answer");
        assert!(quiet.get("files").is_none(), "an item with no files says it has some");
    }

    /// The app is sent the words without the files' lines, and whom an
    /// answer or work is for by name; a person's message is for no one.
    #[test]
    fn the_app_is_sent_words_and_whom_an_answer_is_for() {
        let names: BTreeMap<String, String> = [("mo".to_string(), "Mo".to_string())].into();
        let said = bare("message", "m1".into(), "sam".into(), None, "see this\n📎 plan.pdf\nand that".into(), None, at());
        let answer = bare("answer", "a1".into(), crate::agent_id::of("mo"), Some("mo".into()), "done".into(), None, at());
        let out = for_the_app(vec![said, answer], &names);
        assert_eq!(out[0].text, "see this\nand that", "a file's line is still in the words");
        assert_eq!(out[0].files.as_ref().map(Vec::len), Some(1), "the files went with the lines");
        assert_eq!(out[0].for_name, None, "a person's message is for someone");
        let v = out[1].value();
        assert_eq!((&v["for"], &v["requester"]), (&serde_json::json!("Mo"), &serde_json::json!("mo")), "{v}");
    }

    /// Work the agent began itself is the personal chat's `tasks` entry, its
    /// row a personal task row with every field the personal one has.
    #[test]
    fn work_with_no_message_is_the_personal_chats_tasks_entry() {
        let w = Work {
            task: "t1".into(),
            requester: "mo".into(),
            status: Status::Suspended,
            steps: 3,
            doing: Some("team_read_knowledge".into()),
            title: "Following up".into(),
            long: false,
            thread: None,
            asked_in: None,
            started: at(),
            reason: Some("approval".into()),
            asked: None,
        };
        let v = work(&w).value();
        assert_eq!((&v["kind"], &v["from_run"]), (&serde_json::json!("tasks"), &serde_json::json!("t1")), "{v}");
        let row = &v["tasks"][0];
        for key in ["task_id", "title", "text", "status", "waiting_on", "waiting_item", "status_line"] {
            assert!(row.get(key).is_some(), "a team task row has no {key}: {row}");
        }
        assert_eq!(
            (&row["task_id"], &row["status"], &row["waiting_on"], &row["requester"]),
            (&serde_json::json!("t1"), &serde_json::json!("suspended"), &serde_json::json!("approval"), &serde_json::json!("mo"))
        );
        assert_eq!(row["status_line"], serde_json::Value::Null, "work waiting on an ask says what it last did");
        let running = Work { status: Status::Running, ..w };
        assert_eq!(work(&running).value()["tasks"][0]["status_line"], "Reading the knowledge base", "the app is not told what the work is doing");
    }
}
