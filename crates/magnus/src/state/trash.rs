//! The trash: items out of every view until restored or removed for good,
//! and the conversations removed for good, each with whose it was.
//!
//! Trashing moves owned records across domains (a task with its run and
//! items, a conversation's turns and branch, a library record), so taking
//! and putting back stay orchestrations on the state, through each
//! domain's own take and put methods; this ledger holds what was taken.

use super::{Branch, Item, KnowledgeEntry, LibFile, MemoryEntry, Project, Run, State, Task};
use crate::forget::Erased;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

/// An item in the trash: out of every view until it is restored or removed.
#[derive(Clone, Debug)]
pub struct Trashed {
    pub person: String,
    /// Its title, when the state has it; else `title_from` says where it is
    /// recalled from.
    pub title: String,
    pub title_from: Option<TitleFrom>,
    pub at: DateTime<Utc>,
    pub(super) held: Held,
}

/// Where a trashed item's title is recalled from: the event that holds a
/// task's words, its title (else its text), or its text alone.
#[derive(Clone, Copy, Debug)]
pub enum TitleFrom {
    Title(i64),
    Text(i64),
}

impl TitleFrom {
    pub fn seq(&self) -> i64 {
        match self {
            TitleFrom::Title(s) | TitleFrom::Text(s) => *s,
        }
    }

    /// The title, from the recalled content of `seq()`.
    pub fn title(&self, words: &Value) -> String {
        let text = words["text"].as_str().unwrap_or_default();
        match self {
            TitleFrom::Title(_) => words["title"].as_str().filter(|t| !t.is_empty()).unwrap_or(text).to_string(),
            TitleFrom::Text(_) => text.to_string(),
        }
    }
}

/// What a trashed item took out of the state, to put back if restored. A
/// chat message or answer takes nothing: its thread leaves it out.
#[derive(Clone, Debug)]
pub(super) enum Held {
    Task(Box<Task>, Option<Box<Run>>, Vec<Item>),
    Thread { id: String, turns: Vec<(Task, Option<Run>)>, branch: Option<Branch> },
    Entry { thread: String },
    File(Box<LibFile>),
    Project(Project),
    Memory(MemoryEntry),
    Knowledge(Box<KnowledgeEntry>),
}

impl Trashed {
    /// A trashed memory's record: its content is recalled, not kept.
    pub fn memory(&self) -> Option<&MemoryEntry> {
        match &self.held {
            Held::Memory(m) => Some(m),
            _ => None,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self.held {
            Held::Task(..) => "task",
            Held::Thread { .. } => "conversation",
            Held::Entry { .. } => "message",
            Held::File(_) => "file",
            Held::Project(_) => "project",
            Held::Memory(_) => "memory",
            Held::Knowledge(_) => "knowledge",
        }
    }

    /// The thread a trashed message or answer was in.
    pub fn thread(&self) -> Option<&str> {
        match &self.held {
            Held::Entry { thread } => Some(thread),
            _ => None,
        }
    }
}

/// Why an item cannot go to the trash.
#[derive(Debug, PartialEq)]
pub enum NotTrashable {
    /// No such item of the person's.
    NotFound,
    /// A task still running: cancel it first, so a crash can resume it (C3).
    Running,
}

#[derive(Default, Debug)]
pub struct Trash {
    /// Trashed items by id.
    items: BTreeMap<String, Trashed>,
    /// Conversations removed from the trash, each with whose it was, so
    /// erasing them forgets it.
    removed_threads: HashMap<String, String>,
}

impl crate::forget::Forget for Trash {
    fn forget(&mut self, who: &Erased) {
        let p = who.person.as_str();
        self.items.retain(|_, t| t.person != p);
        // What they removed of their own conversations.
        self.removed_threads.retain(|t, person| person != p && !who.threads.contains(t));
    }
}

impl Trash {
    pub(super) fn put(&mut self, item: String, t: Trashed) {
        self.items.insert(item, t);
    }

    pub(super) fn take(&mut self, item: &str) -> Option<Trashed> {
        self.items.remove(item)
    }

    /// Removed for good: a conversation is marked gone; a message or
    /// answer gives back the thread to hide it in.
    pub(super) fn remove(&mut self, item: &str) -> Option<String> {
        let t = self.items.remove(item)?;
        match t.held {
            Held::Entry { thread } => Some(thread),
            Held::Thread { id, .. } => {
                self.removed_threads.insert(id, t.person);
                None
            }
            _ => None,
        }
    }

    pub fn get(&self, item: &str) -> Option<&Trashed> {
        self.items.get(item)
    }

    pub fn contains(&self, item: &str) -> bool {
        self.items.contains_key(item)
    }

    /// Everything in the trash, by id.
    pub fn items(&self) -> &BTreeMap<String, Trashed> {
        &self.items
    }

    /// A conversation in the trash or removed for good.
    pub fn gone(&self, thread: &str) -> bool {
        self.items.contains_key(thread) || self.removed_threads.contains_key(thread)
    }

    #[cfg(test)]
    pub(crate) fn mark_removed(&mut self, thread: &str, person: &str) {
        self.removed_threads.insert(thread.into(), person.into());
    }

    #[cfg(test)]
    pub(crate) fn canonical_fields(&self) -> Vec<(&'static str, String)> {
        vec![("trash", format!("{:?}", self.items)), ("removed_threads", format!("{:?}", self.removed_threads))]
    }
}

impl State {
    /// Take an item out of every view, into the trash.
    pub(super) fn take(&mut self, item: &str) -> Option<(String, Option<TitleFrom>, Held)> {
        if let Some((_, task)) = item.split_once(':') {
            let t = self.tasks.get(task)?;
            let thread = t.chat.as_ref()?.thread.clone();
            // A message's title is what the person said, recalled when listed.
            let opener = t.chat.as_ref().and_then(|c| c.opener.as_deref()).map(crate::openers::title);
            let (title, from) = match item.starts_with("m:") {
                // Magnus's own opening: its text is the appliance's, not theirs.
                true if opener.is_some() => (opener.unwrap_or_default(), None),
                true => (String::new(), Some(TitleFrom::Text(t.seq))),
                false => (self.runs.get(&t.run_id).and_then(|r| r.summary.clone()).unwrap_or_default(), None),
            };
            return Some((title, from, Held::Entry { thread }));
        }
        if item.starts_with("task_") {
            let t = self.tasks.remove(item)?;
            let r = self.runs.remove(&t.run_id).map(Box::new);
            let items = self.inbox.take_items_of(item);
            return Some((String::new(), Some(TitleFrom::Title(t.seq)), Held::Task(Box::new(t), r, items)));
        }
        if item.starts_with("thread_") {
            let ids: Vec<String> = self.tasks.values().filter(|t| t.chat.as_ref().is_some_and(|c| c.turn && c.thread == item)).map(|t| t.task_id.clone()).collect();
            let mut turns: Vec<(Task, Option<Run>)> = ids
                .iter()
                .filter_map(|id| self.tasks.remove(id))
                .map(|t| {
                    let r = self.runs.remove(&t.run_id);
                    (t, r)
                })
                .collect();
            turns.sort_by_key(|(t, _)| t.created_at);
            let branch = self.thread_extras.take_branch(item);
            // A conversation's title is its first message, recalled when
            // listed; one Magnus opened is called what the app calls it, not
            // by the appliance's instruction (review of #274).
            let (title, from) = match turns.first() {
                Some((t, _)) => match t.chat.as_ref().and_then(|c| c.opener.as_deref()) {
                    Some(o) => (crate::openers::title(o), None),
                    None => (String::new(), Some(TitleFrom::Text(t.seq))),
                },
                None => (branch.as_ref()?.title.clone(), None),
            };
            return Some((title, from, Held::Thread { id: item.into(), turns, branch }));
        }
        if let Some(f) = self.library.take_file(item) {
            return Some((f.name.clone(), None, Held::File(Box::new(f))));
        }
        if let Some(p) = self.library.take_project(item) {
            return Some((p.name.clone(), None, Held::Project(p)));
        }
        // Its title in the trash is its fact, recalled when listed.
        if let Some(m) = self.library.take_memory(item) {
            return Some((String::new(), None, Held::Memory(m)));
        }
        let k = self.library.take_knowledge(item)?;
        Some((k.title.clone(), None, Held::Knowledge(Box::new(k))))
    }

    /// Which events hold a trashed item's own content. A task's or a
    /// conversation's runs keep what they made that is an item of its own:
    /// files they delivered, memories and playbook items they wrote.
    /// `fact` is a trashed memory's fact, recalled by the caller.
    pub fn holding(&self, item: &str, fact: Option<&str>) -> Option<crate::log::Holding> {
        use crate::log::Holding;
        const OWN: [&str; 3] = ["output", "memory", "playbook"];
        let named = |kinds: Vec<&'static str>, field: &'static str, id: &str| Holding { kinds, field, id: id.into(), ..Default::default() };
        // What belongs to a task but carries no run: its verdict's note and
        // the person's answers to its cards.
        let theirs = |task: &str, items: &mut dyn Iterator<Item = &Item>| -> Vec<(&'static str, &'static str, String)> {
            let mut m = vec![("verdict", "task", task.to_string())];
            m.extend(items.map(|i| ("item_resolved", "id", i.id.clone())));
            m
        };
        let items_of = |task: &str| self.inbox.items().values().filter(|i| i.task.as_deref() == Some(task)).cloned().collect::<Vec<_>>();
        Some(match &self.trash.get(item)?.held {
            Held::Task(t, _, items) => Holding { runs: vec![t.run_id.clone()], except: OWN.to_vec(), matches: theirs(&t.task_id, &mut items.iter()), ..Default::default() },
            Held::Thread { id, turns, .. } => {
                let mut matches: Vec<_> = turns.iter().flat_map(|(t, _)| theirs(&t.task_id, &mut items_of(&t.task_id).iter())).collect();
                // The reasons the person gave declining the cards in it.
                let cards = turns.iter().filter_map(|(_, r)| r.as_ref()).flat_map(|r| r.results.values().filter_map(|x| x.data.as_ref()?["card"].as_str().map(String::from)));
                matches.extend(cards.map(|c| ("card_declined", "card", c)));
                Holding { runs: turns.iter().map(|(t, _)| t.run_id.clone()).collect(), except: OWN.to_vec(), matches, ..named(vec!["thread_branched"], "thread", id) }
            }
            Held::Entry { .. } => match item.split_once(':')? {
                ("m", task) => named(vec!["task_created"], "task", task),
                (_, task) => Holding {
                    runs: vec![self.tasks.get(task)?.run_id.clone()],
                    except: [&OWN[..], &["task_created"]].concat(),
                    matches: theirs(task, &mut items_of(task).iter()),
                    ..Default::default()
                },
            },
            Held::File(_) => named(vec!["file_uploaded", "output"], "id", item),
            Held::Project(_) => named(vec!["project_set"], "id", item),
            // A memory and where it was written: the call that wrote it, and
            // the reply that made the call (the run's model, or the memory
            // pass), found by the fact's text in that run.
            Held::Memory(m) => {
                let fact = fact.unwrap_or_default();
                let text = serde_json::to_string(fact).unwrap_or_default();
                let text = text.trim_matches('"').to_string();
                let containing =
                    ["intent", "tool_result", "model_reply", "memory_pass"].into_iter().map(|k| (k, m.run.clone(), text.clone())).filter(|_| !fact.trim().is_empty()).collect();
                Holding { containing, ..named(vec!["memory"], "id", item) }
            }
            Held::Knowledge(_) => named(vec!["knowledge_added"], "id", item),
        })
    }

    /// A trashed item back where it was.
    pub(super) fn put_back(&mut self, held: Held) {
        match held {
            Held::Task(t, r, items) => {
                if let Some(r) = r {
                    self.runs.insert(r.id.clone(), *r);
                }
                for i in items {
                    self.inbox.put_item(i);
                }
                self.tasks.insert(t.task_id.clone(), *t);
            }
            Held::Thread { id, turns, branch } => {
                for (t, r) in turns {
                    if let Some(r) = r {
                        self.runs.insert(r.id.clone(), r);
                    }
                    self.tasks.insert(t.task_id.clone(), t);
                }
                if let Some(b) = branch {
                    self.thread_extras.put_branch(id, b);
                }
            }
            Held::Entry { .. } => {}
            Held::File(f) => self.library.put_file(*f),
            Held::Project(p) => self.library.put_project(p),
            Held::Memory(m) => self.library.put_memory(m),
            Held::Knowledge(k) => self.library.put_knowledge(*k),
        }
    }
}
