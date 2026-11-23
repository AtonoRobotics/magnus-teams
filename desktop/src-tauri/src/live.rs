//! A task's live text, as the page follows it. The chat and the side panel
//! may follow the same task at once: there is one stream per task, and each
//! listener in the page joins it. A listener that joins late is sent what
//! has streamed so far, addressed to it alone, so no one else sees it twice;
//! the stream stops only when its last listener leaves.
use std::collections::{BTreeSet, HashMap};
use std::sync::{Mutex, MutexGuard, PoisonError};
use tokio::task::AbortHandle;

#[derive(Default)]
struct Stream {
    listeners: BTreeSet<String>,
    /// Everything it has streamed since it started.
    text: String,
    /// The job that reads it from the server, once started.
    job: Option<AbortHandle>,
}

/// What joining a task's stream asks of the caller.
#[derive(Debug, PartialEq)]
pub enum Joined {
    /// Nothing streams the task yet: start the job, and hand it over (`started`).
    Start,
    /// It streams already; what it had was sent to the new listener.
    Following,
}

#[derive(Default)]
pub struct Streams(Mutex<HashMap<String, Stream>>);

impl Streams {
    fn all(&self) -> MutexGuard<'_, HashMap<String, Stream>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `listener` follows `task`. When the task already streams, what it has
    /// streamed so far goes to `catch_up`, under the same lock as each later
    /// piece is sent, so the new listener gets it first.
    pub fn join(&self, task: &str, listener: &str, catch_up: impl FnOnce(&str)) -> Joined {
        let mut all = self.all();
        if let Some(s) = all.get_mut(task) {
            if s.listeners.insert(listener.to_string()) && !s.text.is_empty() {
                catch_up(&s.text);
            }
            return Joined::Following;
        }
        let mut s = Stream::default();
        s.listeners.insert(listener.to_string());
        all.insert(task.to_string(), s);
        Joined::Start
    }

    /// The job that streams `task` began. False when everyone left before it
    /// could be handed over: the caller stops it.
    pub fn started(&self, task: &str, job: AbortHandle) -> bool {
        match self.all().get_mut(task) {
            Some(s) => {
                s.job = Some(job);
                true
            }
            None => false,
        }
    }

    /// More of `task`'s text: kept for a later listener, and sent to every
    /// listener now by `send` (under the lock, so it keeps its order).
    pub fn push(&self, task: &str, text: &str, send: impl FnOnce(&str)) {
        if let Some(s) = self.all().get_mut(task) {
            s.text.push_str(text);
            send(text);
        }
    }

    /// `listener` stops following `task`. The job to stop when it was the last.
    pub fn leave(&self, task: &str, listener: &str) -> Option<AbortHandle> {
        let mut all = self.all();
        let s = all.get_mut(task)?;
        s.listeners.remove(listener);
        if !s.listeners.is_empty() {
            return None;
        }
        all.remove(task).and_then(|s| s.job)
    }

    /// Every stream ends (the person signed out): the jobs to stop.
    pub fn end(&self) -> Vec<AbortHandle> {
        self.all().drain().filter_map(|(_, s)| s.job).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job() -> AbortHandle {
        tokio::spawn(std::future::pending::<()>()).abort_handle()
    }

    /// The chat and the panel follow one task: the panel joining sends what
    /// streamed so far to the panel only, each later piece reaches both once,
    /// and the panel closing leaves the chat's stream running.
    #[tokio::test]
    async fn two_listeners_share_one_stream_and_neither_sees_text_twice() {
        let s = Streams::default();
        let mut chat = String::new();
        let mut panel = String::new();
        assert_eq!(s.join("t1", "chat", |_| panic!("nothing streamed yet")), Joined::Start);
        assert!(s.started("t1", job()));
        s.push("t1", "Hello", |t| chat.push_str(t));
        assert_eq!(s.join("t1", "panel", |t| panel.push_str(t)), Joined::Following);
        s.push("t1", " world", |t| {
            chat.push_str(t);
            panel.push_str(t);
        });
        assert_eq!((chat.as_str(), panel.as_str()), ("Hello world", "Hello world"), "a listener saw the text twice, or missed some");
        assert!(s.leave("t1", "panel").is_none(), "the panel closing stopped the chat's stream");
        assert!(s.leave("t1", "chat").is_some(), "the last listener left and the stream went on");
        assert_eq!(s.join("t1", "chat", |_| panic!("an ended stream's text was kept")), Joined::Start);
    }

    /// Everyone left before the job was handed over: it is stopped.
    #[tokio::test]
    async fn a_job_no_one_waits_for_is_refused() {
        let s = Streams::default();
        assert_eq!(s.join("t1", "chat", |_| {}), Joined::Start);
        assert!(s.leave("t1", "chat").is_none());
        assert!(!s.started("t1", job()));
    }

    /// Sign-out ends every stream.
    #[tokio::test]
    async fn the_end_stops_every_stream() {
        let s = Streams::default();
        for task in ["t1", "t2"] {
            s.join(task, "chat", |_| {});
            s.started(task, job());
        }
        assert_eq!(s.end().len(), 2);
        assert_eq!(s.join("t1", "chat", |_| {}), Joined::Start, "a stream outlived the end");
    }
}
