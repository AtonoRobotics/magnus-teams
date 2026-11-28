//! Each person's Magnus in a shared conversation: it answers its person when
//! they mention @Magnus, joins on its own where a channel allows it, and
//! gives long work its own thread with live status and the requester named.
//! Each answer is the asker's own agent at work, posting as itself
//! (`agent_id`, `orchestrator`).
use super::chat::{AgentMode, Chat};
use super::{Change, Role, Workspace};
use crate::events::Ev;
use crate::kernel::Kernel;
use anyhow::Result;
use serde::Serialize;
use std::sync::Arc;

type At = chrono::DateTime<chrono::Utc>;

/// Work still going after this long, or past this many steps, is long work
/// and gets its own thread.
const LONG_WORK_AFTER_SECS: i64 = 20;
const LONG_WORK_STEPS: usize = 3;
/// A question left unanswered this long is a thread that went quiet.
const QUIET_THREAD_AFTER_HOURS: i64 = 24;
/// Older than this, a quiet thread is history and not followed up.
const QUIET_THREAD_LOOKBACK_DAYS: i64 = 7;

/// Why a person's agent took a message on.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Why {
    /// Asked outright: by an older client, or in a chat with one's own Magnus.
    Asked,
    Mentioned,
    /// The channel lets it join when it can help, and this is such a time.
    Helpful,
}

/// Whether a message is explicitly addressed to the agent via `@Magnus`.
/// Bare occurrences of the word "Magnus" never start a task unasked.
pub fn addressed(text: &str) -> Option<Why> {
    let lower = text.to_lowercase();
    let name = crate::agent_id::MENTION.to_lowercase();
    for (i, _) in lower.match_indices(&name) {
        let before = lower[..i].chars().next_back();
        let after = lower[i + name.len()..].chars().next();
        if after.is_some_and(char::is_alphanumeric) {
            continue;
        }
        if before == Some('@') {
            return Some(Why::Mentioned);
        }
    }
    None
}

/// A message the agent can help with unasked: someone catching up, or a
/// question put to the room.
pub fn helpful_cue(text: &str) -> bool {
    const CUES: &[&str] = &[
        "catch me up",
        "what did i miss",
        "what have i missed",
        "can someone",
        "could someone",
        "can somebody",
        "could somebody",
        "can anyone",
        "could anyone",
        "does anyone",
        "does anybody",
        "anyone know",
        "who knows",
        "tl;dr",
    ];
    let lower = text.to_lowercase();
    CUES.iter().any(|c| lower.contains(c))
}

/// Whether work is long: still going after a while, or many steps in.
pub fn is_long(started: At, until: At, steps: usize) -> bool {
    steps >= LONG_WORK_STEPS || (until - started).num_seconds() >= LONG_WORK_AFTER_SECS
}

/// Where a request's work and answer show: in the thread it was asked in;
/// otherwise long work gets a thread on the asking message, and a quick
/// answer stays in the channel.
pub fn placement(asked_in: Option<&str>, asked_message: Option<&str>, long: bool) -> Option<String> {
    asked_in.or(asked_message.filter(|_| long)).map(str::to_string)
}

impl Workspace {
    /// Whether the team agent takes this message on, and why. A guest can
    /// chat with it in view but cannot have it act.
    pub fn agent_answers(&self, channel: &str, sender: Role, text: &str) -> Option<Why> {
        if sender == Role::Guest || !self.channels.contains_key(channel) {
            return None;
        }
        // A chat with Magnus from before each person had their own agent is
        // still that person asking their own Magnus.
        if self.chat.direct.get(channel).is_some_and(|d| d.agent) {
            return Some(Why::Asked);
        }
        addressed(text).or_else(|| (self.agent_mode(channel) == AgentMode::WhenHelpful && helpful_cue(text)).then_some(Why::Helpful))
    }

    /// Threads that went quiet on a member's question, in channels that let
    /// the agent join: the channel, the thread's first message, who asked and
    /// what they asked.
    pub fn quiet_threads(&self, now: At) -> Vec<(String, String, String, String)> {
        let mut out = vec![];
        for (id, ch) in &self.channels {
            if self.agent_mode(id) != AgentMode::WhenHelpful || self.chat.direct.contains_key(id) {
                continue;
            }
            for root in ch.messages.iter().filter(|m| m.thread.is_none()) {
                if self.chat.followed_up.contains(&root.id) {
                    continue;
                }
                let Some(last) = ch.messages.iter().filter(|m| m.id == root.id || m.thread.as_deref() == Some(&root.id)).max_by_key(|m| m.at) else { continue };
                let age = now - last.at;
                let member = self.members.get(&last.sender).is_some_and(|r| *r > Role::Guest);
                if member
                    && last.text.trim_end().ends_with('?')
                    && age >= chrono::Duration::hours(QUIET_THREAD_AFTER_HOURS)
                    && age < chrono::Duration::days(QUIET_THREAD_LOOKBACK_DAYS)
                {
                    out.push((id.clone(), root.id.clone(), last.sender.clone(), last.text.clone()));
                }
            }
        }
        out
    }
}

impl Kernel {
    /// Follows up once on each thread that went quiet, for the member whose
    /// question was left: their own agent works on it in the background, and
    /// its answer waits in their Release Airlock, as when it is only named,
    /// to post in the thread once they release it. Its objective carries
    /// only their own question, never the thread.
    pub(super) async fn follow_up_quiet_threads(self: &Arc<Self>) -> Result<()> {
        let now = chrono::Utc::now();
        let quiet: Vec<(String, String, String, String, String)> =
            self.read(|s| s.team.all().flat_map(|w| w.quiet_threads(now).into_iter().map(|(c, t, p, q)| (w.id.clone(), c, t, p, q))).collect());
        for (workspace, channel, thread, person, question) in quiet {
            self.emit("poll", Ev::Team(Change::Chat { workspace: workspace.clone(), chat: Chat::FollowedUp { channel: channel.clone(), thread: thread.clone() } })).await?;
            let objective = follow_up_objective(&question);
            let pick = crate::say::Pick { model: None, reasoning: None };
            if let Err(e) = self.reply_in_room_as("poll", &workspace, &channel, &person, &objective, Some(thread), None, pick, true, None).await {
                tracing::warn!("team follow-up for {person} in {workspace}: {e:#}");
            }
        }
        Ok(())
    }
}

/// What a follow-up asks the person's own agent: their question, cut to
/// what one lane's objective holds.
fn follow_up_objective(question: &str) -> String {
    const ASK: &str = "A question you asked in a shared conversation went a day without an answer. Answer it if you can, or say who could:\n\n";
    let room = super::orchestrator::MAX_OBJECTIVE_CHARS.saturating_sub(ASK.chars().count());
    format!("{ASK}{}", question.trim().chars().take(room).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::super::{Channel, Message};
    use super::*;

    fn ws(mode: AgentMode) -> Workspace {
        let mut w = Workspace::default();
        w.members.insert("alice".into(), Role::Owner);
        w.members.insert("gus".into(), Role::Guest);
        w.channels.insert("c".into(), Channel::default());
        w.chat.modes.insert("c".into(), mode);
        w
    }

    #[test]
    fn it_answers_only_when_explicitly_mentioned() {
        let w = ws(AgentMode::Mentions);
        assert_eq!(w.agent_answers("c", Role::Member, "@Magnus what's left?"), Some(Why::Mentioned));
        assert_eq!(w.agent_answers("c", Role::Member, "magnus, summarize this"), None, "bare word does not start a task");
        assert_eq!(w.agent_answers("c", Role::Member, "Thanks Magnus."), None, "bare word does not start a task");
        assert_eq!(w.agent_answers("c", Role::Member, "lunch at noon?"), None);
        assert_eq!(w.agent_answers("c", Role::Member, "my Magnus has notes on this"), None, "a personal agent is not the team's");
        assert_eq!(w.agent_answers("c", Role::Member, "the magnusson file"), None);
    }

    #[test]
    fn a_guest_cannot_have_it_act() {
        assert_eq!(ws(AgentMode::WhenHelpful).agent_answers("c", Role::Guest, "@Magnus send it"), None);
    }

    #[test]
    fn mentions_only_stays_quiet_where_join_when_helpful_joins() {
        let ask = "Can someone pull together what the testers reported?";
        assert_eq!(ws(AgentMode::Mentions).agent_answers("c", Role::Member, ask), None);
        assert_eq!(ws(AgentMode::WhenHelpful).agent_answers("c", Role::Member, ask), Some(Why::Helpful));
        assert_eq!(ws(AgentMode::WhenHelpful).agent_answers("c", Role::Member, "I'll take that one"), None);
    }

    #[test]
    fn an_old_chat_with_magnus_is_still_answered() {
        let mut w = ws(AgentMode::Mentions);
        w.chat.direct.insert("c".into(), super::super::chat::Direct { people: vec!["alice".into()], agent: true });
        assert_eq!(w.agent_answers("c", Role::Owner, "draft the update"), Some(Why::Asked));
    }

    #[test]
    fn a_new_channel_is_mentions_only() {
        assert_eq!(Workspace::default().agent_mode("c"), AgentMode::Mentions);
    }

    #[test]
    fn long_work_gets_its_own_thread_and_a_quick_answer_stays_put() {
        let t0 = chrono::Utc::now();
        assert!(!is_long(t0, t0 + chrono::Duration::seconds(3), 1));
        assert!(is_long(t0, t0 + chrono::Duration::seconds(LONG_WORK_AFTER_SECS), 1));
        assert!(is_long(t0, t0, LONG_WORK_STEPS));
        assert_eq!(placement(None, Some("m"), true), Some("m".to_string()));
        assert_eq!(placement(None, Some("m"), false), None);
        assert_eq!(placement(Some("t"), Some("m"), true), Some("t".to_string()), "asked in a thread, it stays in that thread");
    }

    fn said(id: &str, by: &str, text: &str, thread: Option<&str>, at: At) -> Message {
        Message { id: id.into(), sender: by.into(), text: text.into(), thread: thread.map(str::to_string), at }
    }

    #[test]
    fn a_quiet_question_is_followed_up_once_where_the_channel_allows() {
        let now = chrono::Utc::now();
        let old = now - chrono::Duration::hours(QUIET_THREAD_AFTER_HOURS + 1);
        let mut w = ws(AgentMode::WhenHelpful);
        let msgs = &mut w.channels.get_mut("c").unwrap().messages;
        msgs.push(said("q", "alice", "Who has the deck?", None, old));
        msgs.push(said("r", "alice", "Plan is set.", None, old));
        msgs.push(said("g", "gus", "Any news?", None, old));
        msgs.push(said("n", "alice", "Ready?", None, now));
        assert_eq!(w.quiet_threads(now), vec![("c".to_string(), "q".to_string(), "alice".to_string(), "Who has the deck?".to_string())]);
        w.chat.followed_up.insert("q".into());
        assert!(w.quiet_threads(now).is_empty());
        w.chat.followed_up.clear();
        w.chat.modes.insert("c".into(), AgentMode::Mentions);
        assert!(w.quiet_threads(now).is_empty(), "mentions only never joins on its own");
    }
}
