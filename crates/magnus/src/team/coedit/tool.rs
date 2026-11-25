//! A person's own agent and a shared project's files: it reads one
//! (`team_read_file`), and proposes an edit to a co-edited one
//! (`team_propose_edit`) as part of its lane's result.
//!
//! Reading taints the run when anyone else wrote part of the file, the way
//! another member's channel message does (`owner::from_others`). An edit
//! is made only in a lane, a request in the project's room: it releases the
//! person's work to the project, so it goes with the lane's result through
//! the release gate (`lane_release`) and lands only when that is released.

use super::commit::Draft;
use super::{merge, Author, Landed};
use crate::boundary::Action;
use crate::kernel::Kernel;
use crate::state::{Intent, Run};
use crate::team::owner::from_others;
use anyhow::{Context, Result};
use serde_json::{json, Value};

/// How much of a file one read returns.
const MAX_READ_CHARS: usize = 100_000;
/// Why an edit outside a lane is refused.
const NOT_IN_A_LANE: &str = "an agent edits a co-edited file only while working on a request in its project's room, where the edit goes with the request's result for release";

/// What a read result says of a co-edited file it returned.
const READ_KEY: &str = "coedit";

impl Kernel {
    /// `team_read_file`: a shared file of a workspace the person is in.
    pub(crate) async fn team_read_file(&self, a: &Action) -> Result<(String, bool, Option<Value>)> {
        let run = self.read(|s| s.runs.get(&a.run).cloned()).context("no such run")?;
        let (ws, name) = (a.input["workspace"].as_str().unwrap_or_default(), a.input["name"].as_str().unwrap_or_default());
        let seen = self.read(|s| {
            let w = s.team.workspace(ws).filter(|w| w.sees_file(name, &run.person))?;
            let co = w.coedit.files.get(name).map(|f| (f.format, f.head().map_or(0, |h| h.number), f.writers().map(String::from).collect::<Vec<_>>()));
            Some((w.files.get(name).map(|f| f.by.clone()).unwrap_or_default(), co))
        });
        let Some((by, co)) = seen else { return Ok(("there is no such file open to you".into(), true, None)) };
        let (bytes, writers, head) = match &co {
            Some((_, head, writers)) => (self.coedit_bytes(ws, name, *head).await?.unwrap_or_default(), writers.clone(), Some(*head)),
            None => (tokio::fs::read(crate::team::agent::volume_dir(self, ws).join(name)).await.unwrap_or_default(), vec![by], None),
        };
        // Words anyone else wrote are data to this run, never its person's instruction.
        let untrusted = from_others(&run.person, writers.iter().map(String::as_str));
        let format = co.as_ref().map_or(merge::Format::of(name), |(f, _, _)| *f);
        let mut text = match head {
            Some(n) => format!("{name}, version {n}, co-edited. To change it, propose the whole new file with team_propose_edit, base {n}.\n\n"),
            None => format!("{name}, a shared copy (read only).\n\n"),
        };
        match std::str::from_utf8(&bytes) {
            Ok(body) if format != merge::Format::Binary || head.is_none() => {
                text.extend(body.chars().take(MAX_READ_CHARS));
                if let Some(values) = merge::values(format, &bytes) {
                    text.push_str(&format!("\n\nIts values, formulas worked out:\n{}", values.chars().take(MAX_READ_CHARS).collect::<String>()));
                }
            }
            _ => text.push_str(&format!("A binary file of {} bytes; its content is not shown here.", bytes.len())),
        }
        let mut data = json!({ "untrusted": untrusted, "private": true });
        if let Some(n) = head {
            data[READ_KEY] = json!({ "workspace": ws, "name": name, "version": n });
        }
        Ok((text, false, Some(data)))
    }

    /// `team_propose_edit`: the whole new file, made on version `base`.
    pub(crate) async fn team_propose_edit(&self, a: &Action) -> Result<(String, bool, Option<Value>)> {
        let run = self.read(|s| s.runs.get(&a.run).cloned()).context("no such run")?;
        let (ws, name) = (a.input["workspace"].as_str().unwrap_or_default(), a.input["name"].as_str().unwrap_or_default());
        let (Some(base), Some(content)) = (a.input["base"].as_u64().and_then(|b| u32::try_from(b).ok()), a.input["content"].as_str()) else {
            return Ok(("base (the version you read) and content (the whole new file) are required".into(), true, None));
        };
        let Some((mission, lane)) = self.lane_of(&run, ws) else { return Ok((NOT_IN_A_LANE.into(), true, None)) };
        if !self.read(|s| s.team.workspace(ws).is_some_and(|w| w.sees_file(name, &run.person))) {
            return Ok(("there is no such file open to you".into(), true, None));
        }
        let author = Author { person: run.person.clone(), run: Some(run.id.clone()) };
        let draft = Draft { ws, name, base, mine: content.as_bytes(), author: &author };
        let said = match self.coedit_propose(&format!("person:{}", run.person), draft, &mission, &lane).await? {
            Landed::Proposed(id) => {
                let text = format!("Proposed as edit {id}, made on version {base} of {name}. It lands with your result: when that is released to the room, it is committed under the version check.");
                return Ok((text, false, Some(json!({ "untrusted": false, "private": true }))));
            }
            Landed::Rejected { head, conflicts, .. } => {
                let parts: String =
                    conflicts.iter().map(|c| format!("\n- {}: yours {:?}; now {:?}", c.at, c.mine.as_deref().unwrap_or(""), c.theirs.as_deref().unwrap_or(""))).collect();
                format!("Rejected: {name} is at version {head}, and these parts of yours conflict with what landed since:{parts}\nNothing is lost. Read it again with team_read_file, make your change on version {head}, and propose again.")
            }
            Landed::Version(n) => format!("{name} is at version {n}."),
            Landed::Refused(why) => why,
        };
        Ok((said, true, None))
    }

    /// The mission and lane this run (or the run it is a sub-agent of) works
    /// on for its person, in `ws`.
    fn lane_of(&self, run: &Run, ws: &str) -> Option<(String, String)> {
        self.read(|s| {
            let top = s.sub_agents.top(&run.id).unwrap_or_else(|| run.id.clone());
            let task = s.runs.get(&top)?.task.clone();
            let m = s.missions.of_member_lane(&task).filter(|m| m.workspace == ws)?;
            let l = m.lanes.values().find(|l| l.child_task.as_deref() == Some(task.as_str()) && l.owner.as_deref() == Some(run.person.as_str()))?;
            Some((m.id.clone(), l.id.clone()))
        })
    }
}

/// The co-edited files' versions a send carried, for its receipt: each
/// file this run read before the send, whose name the send's input names,
/// at the version it read last.
pub(crate) fn versions_sent(run: &Run, sent: &Intent) -> Vec<Value> {
    let input = sent.input.to_string();
    let mut reads: Vec<&Intent> = run.intents.values().filter(|i| i.tool == "team_read_file" && i.at <= sent.at && i.call_id != sent.call_id).collect();
    reads.sort_by_key(|i| i.at);
    let mut out: Vec<Value> = vec![];
    for r in reads {
        let Some(read) = r.result.as_ref().filter(|x| !x.is_error).and_then(|x| x.data.as_ref()).map(|d| &d[READ_KEY]).filter(|d| d.is_object()) else { continue };
        let name = read["name"].as_str().unwrap_or_default();
        if name.is_empty() || !input.contains(name) {
            continue;
        }
        out.retain(|v| v["name"] != read["name"] || v["workspace"] != read["workspace"]);
        out.push(read.clone());
    }
    out
}
