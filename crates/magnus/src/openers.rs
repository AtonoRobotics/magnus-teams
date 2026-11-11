//! Openers (behavior spec, Suggestions and proposals: "at cold start, and
//! for each newly connected app, Magnus looks at what it may see and
//! proposes two or three things it could take over"; behavior.md step 4,
//! #255). Magnus starts a chat turn itself, in a new conversation of the
//! person's: at their cold start (a sign-in while they have no conversation
//! or task here yet), and when they connect an app for the first time.
//! Its text is the appliance's instruction (`instructions/opener.md`,
//! `opener-app.md`), with no taint of its own, recorded as an opener so the
//! thread and later turns never take it for the person's words. Its cards
//! are a chat turn's like any other: the evidence check, the ledger and
//! `decide_card` apply unchanged. Each starts once ever (a derived id), off
//! the request that caused it, and not while the kill switch is on.

use crate::chat::derived;
use crate::events::{ChatOrigin, Opener};
use crate::kernel::{Kernel, NewRun};
use crate::run::{OPENER, OPENER_APP};
use std::sync::Arc;

/// One opener starts at a time, so two sign-ins at once start one.
static STARTING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// What a conversation Magnus opened is called.
pub fn title(o: &Opener) -> String {
    match o.app_title.as_deref().or(o.app.as_deref()) {
        Some(app) => format!("What I could take over in {app}"),
        None => "What I could take over".into(),
    }
}

impl Kernel {
    /// After a sign-in: the person's cold start, when they have nothing here yet.
    pub fn open_after_sign_in(self: &Arc<Self>, person: &str) {
        if !self.read(|s| s.tasks.values().any(|t| t.person == person) || s.branches.values().any(|b| b.person == person)) {
            self.open(person, None);
        }
    }

    /// After an app is connected: its first connection by this person,
    /// never a reconnection.
    pub fn open_after_connect(self: &Arc<Self>, person: &str, app: &str) {
        if self.read(|s| s.app_connections.get(&(person.to_string(), app.to_string())).copied()) == Some(1) {
            self.open(person, Some(app));
        }
    }

    /// Start it in the background: the sign-in or the connection does not
    /// wait for the model.
    fn open(self: &Arc<Self>, person: &str, app: Option<&str>) {
        if !self.cfg.proposals.openers || self.read(|s| s.kill_switch) {
            return;
        }
        let (k, person, app) = (self.clone(), person.to_string(), app.map(String::from));
        tokio::spawn(async move {
            match k.start_opener(&person, app.as_deref()).await {
                Err(e) if !crate::halts::is_refusal(&e) => tracing::warn!("could not open a conversation for {person}: {e:#}"),
                _ => {}
            }
        });
    }

    async fn start_opener(self: &Arc<Self>, person: &str, app: Option<&str>) -> anyhow::Result<()> {
        let key = match app {
            Some(a) => format!("opener:{person}:app:{a}"),
            None => format!("opener:{person}"),
        };
        let (task, run, thread) = (derived("task", &key), derived("run", &key), derived("thread", &key));
        let _one = STARTING.lock().await;
        // Started before, even if its conversation was deleted since.
        if self.read(|s| s.tasks.contains_key(&task) || s.openers.contains(&(person.to_string(), task.clone()))) {
            return Ok(());
        }
        let app_title = app.map(|a| self.all_connectors().into_iter().find(|(c, _)| c.name == a).and_then(|(c, _)| c.title).unwrap_or_else(|| crate::connectors::title(a)));
        let text = match &app_title {
            Some(t) => OPENER_APP.replace("{app}", t),
            None => OPENER.to_string(),
        };
        let opener = Opener { app: app.map(String::from), app_title };
        let o = NewRun {
            chat: Some(ChatOrigin { thread, turn: true, from_run: None, conversation: vec![], opener: Some(Box::new(opener)) }),
            ids: Some((task, run)),
            ..Default::default()
        };
        self.create_run("kernel", person, text, vec![], o).await?;
        Ok(())
    }
}
