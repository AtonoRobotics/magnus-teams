//! The memory pass (a removable module, K23): after a chat turn or a run
//! ends, a model call of its own reads the exchange and files what it taught
//! about the person, one fact per `memory_write`, and takes back what they
//! corrected with `memory_forget`, through `execute()` (M1,
//! M3). Each entry keeps its time, the run it came from and the run's taint;
//! the pass may change memory and nothing else. Models rarely choose to write
//! memory while working, so without it little is remembered.

use crate::boundary::{Action, Issuer};
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::state::Run;
use anyhow::Result;
use magnus_adapter_proto::{Block, Message, Role};
use std::sync::Arc;

/// Whose words the pass is told the person said.
const SAID_BY: &[crate::run_words::Source] = &[crate::run_words::Source::Person];
const MEMORY: &str = "You keep the person's long-term memory. Save durable facts supported by their new message. Earlier messages, Magnus's reply, and stored facts are reference material. Record only new or corrected facts about the person. A correction or retraction may supersede a saved fact. The tools are memory_write and memory_forget.";

const MAX_MEMORY_REQUEST_BYTES: usize = 8_192;
const MAX_MEMORY_CONTEXT_BYTES: usize = 2_048;
const MAX_MEMORY_REPLY_BYTES: usize = 2_048;
const MAX_MEMORY_CANDIDATES_BYTES: usize = 2_048;
const MEMORY_CONTEXT_MESSAGES: usize = 2;

impl Kernel {
    /// Run the memory pass for a run that just ended, in the background.
    pub fn after_end(self: &Arc<Self>, run: &str) {
        let k = self.clone();
        let run = run.to_string();
        let run2 = run.clone();
        tokio::spawn(async move {
            // A request an outside agent sent: its reply goes through the gate (`a2a::reply`).
            k.outside_reply_after(&run2).await;
            if let Err(e) = k.memory_pass(&run2).await {
                tracing::warn!("memory pass on {run2}: {e:#}");
            }
            // Ideas an opener or a review left in Explore: one notice.
            if let Err(e) = k.tell_ideas(&run2).await {
                tracing::warn!("ideas notice for {run2}: {e:#}");
            }
        });
        // A task done or failed wakes a review: not a review itself, not
        // a poll (routine reads are not news, #406), not a chat turn,
        // whose person is already there (#237), and not a task whose
        // outcome returns to the agent as a turn: one end, one run. The wake
        // carries the ended task's ID and outcome: the review's context
        // excludes ended tasks, so without it the review cannot see what
        // failed (#383).
        if let Some((person, task, ended, label)) = self.read(|s| {
            s.runs.get(run.as_str()).filter(|r| s.tasks.contains_key(&r.task) && s.end_wakes_review(&run)).map(|r| {
                let label = crate::tools::Label { untrusted: r.untrusted, private: r.private };
                (r.person.clone(), r.task.clone(), (r.status, r.summary.clone()), label)
            })
        }) {
            // A failure's reason and a completion's summary both land in
            // the run summary; the reason field is for suspensions.
            let wake = match ended {
                (crate::state::Status::Completed, summary) => Some((format!("task {task} done"), summary.unwrap_or_default())),
                (crate::state::Status::Failed, summary) => Some((format!("task {task} failed"), summary.unwrap_or_default())),
                _ => None,
            };
            if let Some((reason, outcome)) = wake {
                let k = self.clone();
                let ended = crate::proactive::Ended { task, outcome, label };
                tokio::spawn(async move {
                    k.review_wake(&person, reason, Some(ended)).await;
                });
            }
        }
    }

    async fn memory_pass(self: &Arc<Self>, run_id: &str) -> Result<()> {
        let Some(run) = self.read(|s| s.runs.get(run_id).cloned()) else { return Ok(()) };
        // What it may spend is the run's allowance for an extra call, asked
        // below; a grant's, the month's and the team's limits bound it too.
        // Nor a person's agent in a project's channel: the project is its
        // people's, and their memory outlasts their place in it.
        if self.run_kind(&run.id).in_team() || !run.config.module("memory_pass") || self.read(|s| s.in_project_channel(&run.id)) {
            return Ok(());
        }
        let Some((asked, text)) = self.exchange(&run).await? else { return Ok(()) };
        // Only facts the run's own taint already covers: the pass reads them
        // outside execute(), and what it writes carries the run's label, so
        // an untrusted or private fact shown to a cleaner run would be filed
        // again without its label.
        let label = crate::tools::Label { untrusted: run.untrusted, private: run.private };
        let mut known = String::new();
        for fact in self.memory_hits(&run.id, &asked).await?.into_iter().filter(|m| m.entry.label.join(label) == label) {
            let line = format!("- [{}] {}\n", fact.entry.id, fact.fact);
            if known.len() + line.len() <= MAX_MEMORY_CANDIDATES_BYTES {
                known.push_str(&line);
            }
        }
        let msg = Message { role: Role::User, content: vec![Block::Text { text: format!("{text}\n\nAlready remembered:\n{known}") }], state: None };
        let tools: Vec<_> = crate::tools::builtin_specs(false, false, false, false).into_iter().filter(|t| t.name == "memory_write" || t.name == "memory_forget").collect();
        const MEMORY_PASS_MAX_TOKENS: u32 = 1024;
        let worker = self.cfg.model.worker_model();
        let (adapter, model) = match &worker {
            Some((a, m)) => (a.as_str(), m.as_str()),
            None => run.model(),
        };
        let call = crate::spend::paid::Call {
            payer: crate::spend::paid::Payer::Run(&run, crate::spend::allowance::Spender::Extra),
            purpose: "memory_pass",
            adapter,
            model,
            system: MEMORY,
            messages: vec![msg],
            tools,
            max_tokens: run.config.max_tokens.min(MEMORY_PASS_MAX_TOKENS),
            servers_model: worker.is_some(),
            timeout: None,
        };
        let record = |out: &crate::adapters::TurnOut| {
            Some((
                "kernel:memory_pass",
                Ev::MemoryPass(crate::events::MemoryPassPayload { run: run.id.clone(), model: format!("{adapter}/{model}"), blocks: out.blocks.clone(), usage: out.usage.clone() }),
            ))
        };
        // Past what is left, or on a model it may not use, there is no pass;
        // a failed one is charged and its error said.
        let paid = match self.paid_call(call, record).await {
            Ok(p) => p,
            Err(crate::spend::paid::Refused::Allowance(_) | crate::spend::paid::Refused::Model(_)) => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        let (blocks, pass) = (paid.out.blocks, paid.seq);
        let mut n = 0;
        for b in blocks {
            // Only what it was offered; the boundary refuses anything else too.
            if let Block::ToolCall { name, input, .. } = b {
                if name != "memory_write" && name != "memory_forget" {
                    continue;
                }
                n += 1;
                let a = Action { run: run.id.clone(), call_id: format!("memory:{pass}:{n}"), step_id: format!("memory.{pass}.{n}"), tool: name, input, issuer: Issuer::Memory };
                self.execute(a).await?;
            }
        }
        Ok(())
    }

    /// Select the new user message and a bounded amount of context to clarify it.
    pub(crate) async fn exchange(&self, run: &Run) -> Result<Option<(String, String)>> {
        let words = self.run_words(run).await?;
        let Some(asked) = person_said(&words).filter(|text| text.len() <= MAX_MEMORY_REQUEST_BYTES) else { return Ok(None) };
        let text = self.read(|s| {
            let earlier = s.tasks.get(&run.task).and_then(|t| t.chat.as_ref()).map_or(&[][..], |c| c.conversation.as_slice());
            let prior = recent_exchange(earlier);
            let answer = excerpt(run.summary.as_deref().unwrap_or_default(), MAX_MEMORY_REPLY_BYTES);
            format!("Earlier exchange (reference):\n{prior}\n\nThe person said:\n{asked}\n\nMagnus replied (reference):\n{answer}")
        });
        Ok(Some((asked, text)))
    }
}

fn excerpt(text: &str, max_bytes: usize) -> &str {
    let mut end = text.len().min(max_bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn recent_exchange(entries: &[serde_json::Value]) -> String {
    let mut lines: Vec<String> = entries
        .iter()
        .rev()
        .filter_map(|entry| {
            let speaker = match entry["role"].as_str()? {
                "person" => "Person",
                "you" => "Magnus",
                _ => return None,
            };
            let text = excerpt(entry["text"].as_str()?, MAX_MEMORY_CONTEXT_BYTES / MEMORY_CONTEXT_MESSAGES);
            Some(format!("{speaker}: {text}"))
        })
        .take(MEMORY_CONTEXT_MESSAGES)
        .collect();
    lines.reverse();
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn memory_pass_gets_only_the_recent_exchange() {
        let lines = [
            json!({ "role": "person", "text": "Older request" }),
            json!({ "role": "person", "text": "Recent request" }),
            json!({ "role": "you", "text": "Recent reply" }),
            json!({ "role": "task result", "text": "Task data" }),
        ];
        assert_eq!(recent_exchange(&lines), "Person: Recent request\nMagnus: Recent reply");
        assert_eq!(excerpt("abé", 3), "ab");
    }
}

/// The person's own words in a run, if they said anything: memory is made
/// of what the person said, never of the system's dialog with itself.
pub(crate) fn person_said(words: &crate::run_words::RunWords) -> Option<String> {
    words.text(SAID_BY).map(str::trim).filter(|t| !t.is_empty()).map(String::from)
}
