//! The one door every automatic lane release goes through: a finished
//! lane's answer that would post itself into a shared room (the asker's
//! own `@Magnus`, a routine's report, an open-web lane, a standing
//! release) leaves its person only past the checks any agent send to other
//! people gets. The scrubber, the run's taint, the outgoing classifier and
//! Auto Review each may hold it; then it waits in its owner's Release
//! Airlock, with the reason, instead of the room. A release the owner makes
//! by hand from the Airlock is their own act and does not come here
//! (`mission_routes::release_lane`). See
//! decisions/2026-11-23-lane-releases-pass-the-boundary.md.
//!
//! The lane's edits to co-edited files (`coedit`) are part of its result:
//! the same checks see them, and they land, under the version check, only
//! when the result is released. One that conflicts by then holds the lane
//! for its owner, who sees both versions and decides.

use super::missions::LaneStep;
use crate::auto_review::Verdict;
use crate::boundary::{Action, Facts, Issuer, Shown};
use crate::compartment::query_gate::scrub_check;
use crate::events::TeamMissionEvent;
use crate::kernel::Kernel;
use crate::state::Run;
use crate::tools::{Audience, Label, Risk};
use serde_json::{json, Value};
use std::sync::Arc;

/// What a lane's release is judged as: a post into a shared room.
pub(crate) const RELEASE_TOOL: &str = "team_lane_release";
/// Why a lane with no owner, whose Airlock there is none, fails when held.
const HELD_WITHOUT_OWNER: &str = "the result could not be shared: a check held it";
/// Why the run's taint holds it, as its owner reads it.
const TAINTED: &str = "your agent read content from outside (an email, a page, a message) while holding your private data, so you decide whether this goes to ";
/// Who reads a release, as Auto Review is told.
const AUDIENCE: &str = "it is posted into a shared room, where the room's other members, who are other people, read it";

/// A finished lane whose answer would post itself.
pub(super) struct Finished<'a> {
    pub workspace: String,
    pub mission: String,
    pub lane: String,
    pub owner: Option<String>,
    pub run: &'a Run,
    pub summary: String,
    pub thread: Option<String>,
}

/// What the checks said: post it, or hold it and why.
#[derive(Debug, PartialEq)]
pub(crate) enum Gate {
    Post,
    Hold { why: String, scrub_flagged: bool },
}

fn hold(why: impl Into<String>) -> Gate {
    Gate::Hold { why: why.into(), scrub_flagged: false }
}

/// A finished answer that would leave its person by itself, as the checks
/// judge it: the run that made it, what it is judged as (`tool` of `app`),
/// who reads it, and what covers it.
pub(crate) struct Send<'a> {
    pub run: &'a Run,
    pub call_id: String,
    pub tool: &'static str,
    pub app: &'static str,
    pub audience: Audience,
    pub input: Value,
    pub readers: (&'static str, Vec<String>),
    pub text: &'a str,
    /// What else goes with the text, shown to Auto Review beside it (a
    /// lane's edits to co-edited files).
    pub content: Vec<(String, String)>,
    /// Where it goes, as its owner reads why the taint held it.
    pub to: &'static str,
    /// Why it may go, as Auto Review is told.
    pub standing: String,
    /// Whether the person set up what covers it (a standing release).
    pub covered: bool,
    /// The shared project it stays in, whose words the run started with
    /// (its room or one of its chats); `None` for an outside agent.
    pub within: Option<&'a str>,
}

/// The checks that rest on no model's word: the scrubber, and the run's
/// taint (it read content from outside while holding private data).
fn first_holds(text: &str, run: &Run, to: &str, within: Option<&str>) -> Option<Gate> {
    if let Some(reason) = scrub_check(text) {
        return Some(Gate::Hold { why: format!("the scrubber flagged it: {reason}"), scrub_flagged: true });
    }
    (run.outside && holds_private_for(run, within)).then(|| hold(format!("{TAINTED}{to}")))
}

/// Whether a run holds private data a send to `within` (its own shared
/// project) or past it may not carry after an outside read. A run asked in
/// a shared chat starts private for the chat's words, so they are never
/// filed as public; those words going back to the same project leak
/// nothing, and neither do the project's own knowledge and files. What
/// holds such a post is private data from anywhere else: a person's app, a
/// named source it started with, or another shared project
/// (2026-11-28-projects-without-hoops).
pub(crate) fn holds_private_for(run: &Run, within: Option<&str>) -> bool {
    let Some(workspace) = within else { return run.private };
    let own = crate::team::project_source(workspace);
    run.read_private || run.private_from.iter().any(|from| !from.is_empty() && *from != own)
}

/// Who asked, as Auto Review weighs it: the owner themselves, the open web
/// for the requester, or a teammate under the owner's standing release.
enum Asked {
    Themselves,
    OpenWeb,
    Standing,
}

impl Asked {
    fn standing(&self) -> String {
        let why = match self {
            Asked::Themselves => "the person asked for this in the room themselves",
            Asked::OpenWeb => "public web research the person asked for in this room",
            Asked::Standing => "a teammate asked; covered by a standing release the person set for this exact request in this room",
        };
        format!("{why}; {AUDIENCE}")
    }
}

impl Kernel {
    /// Release a finished lane's answer into its room when every check lets
    /// it go, or else hold it in its owner's Airlock with the reason (a lane
    /// no one owns fails instead). Every automatic release comes here.
    pub(super) async fn release_automatically(self: &Arc<Self>, w: Finished<'_>) {
        let Finished { workspace, mission, lane, owner, run, summary, thread } = w;
        let mut gate = self.lane_release_checks(&workspace, &mission, &lane, owner.as_deref(), run, &summary).await;
        if gate == Gate::Post {
            if let Some(why) = self.land_lane_edits(&workspace, &mission, &lane).await {
                gate = hold(why);
            }
        }
        match gate {
            Gate::Post => {
                let Some(credit) = self.read(|s| s.missions.release_credit(&mission, &lane)) else {
                    return;
                };
                let released =
                    TeamMissionEvent::LaneReleased { workspace: workspace.clone(), mission: mission.clone(), lane, contribution: summary.clone(), released_by: credit.clone() };
                if self.move_lane("kernel", LaneStep::AutoRelease, released).await.unwrap_or(false) {
                    self.post_released_contribution(&workspace, &mission, &credit, &summary, thread).await;
                    self.refresh_mission_deliverable(&workspace, &mission).await;
                }
            }
            Gate::Hold { why, scrub_flagged } => {
                tracing::info!("lane {lane} of mission {mission} held for its owner: {why}");
                let ev = match owner {
                    Some(owner) => TeamMissionEvent::LaneDraftReady { workspace, mission, lane, owner, draft: summary, scrub_flagged, held_because: Some(why) },
                    None => TeamMissionEvent::LaneFailed { workspace, mission, lane, reason: HELD_WITHOUT_OWNER.into() },
                };
                let step = if matches!(ev, TeamMissionEvent::LaneFailed { .. }) { LaneStep::Fail } else { LaneStep::Draft };
                let _ = self.move_lane("kernel", step, ev).await;
            }
        }
    }

    /// Land the lane's edits to co-edited files, or say why the lane waits.
    async fn land_lane_edits(&self, workspace: &str, mission: &str, lane: &str) -> Option<String> {
        match self.coedit_land_lane("kernel", workspace, mission, lane).await {
            Ok(Ok(_)) => None,
            Ok(Err(stuck)) => {
                let names: Vec<String> = stuck.into_iter().map(|(_, name, _)| name).collect();
                Some(format!("{} changed since your agent's edit, and parts of it conflict: decide them in your Airlock", names.join(", ")))
            }
            Err(e) => {
                tracing::warn!("lane {lane} of mission {mission}: its edits: {e:#}");
                Some("your agent's edits to shared files could not be landed".into())
            }
        }
    }

    /// The checks, in order, each of which may only hold: the scrubber; the
    /// run's taint (it read content from outside while holding private
    /// data), which rests on no model's word; the outgoing classifier; and
    /// Auto Review, judging a post to the room's other members.
    async fn lane_release_checks(&self, workspace: &str, mission: &str, lane: &str, owner: Option<&str>, run: &Run, summary: &str) -> Gate {
        if let Some(g) = first_holds(summary, run, "the room", Some(workspace)) {
            return g;
        }
        // Its edits to co-edited files go out with it, and are judged with it.
        let edits: Vec<(String, String)> = self.read(|s| {
            s.team.workspace(workspace).map(|w| w.coedit.of_lane(mission, lane).into_iter().map(|o| (o.name.clone(), super::coedit::shown(&o.diff))).collect()).unwrap_or_default()
        });
        if let Some((name, reason)) = edits.iter().find_map(|(name, text)| scrub_check(text).map(|r| (name, r))) {
            return Gate::Hold { why: format!("the scrubber flagged your agent's edit to {name}: {reason}"), scrub_flagged: true };
        }
        let place = self.read(|s| {
            let m = s.missions.mission(mission)?;
            let w = s.team.workspace(workspace)?;
            let room = w.channels.get(&m.room)?;
            let poster = owner.unwrap_or(m.requester.as_str());
            let readers: Vec<String> = w.members.keys().filter(|p| p.as_str() != poster && w.sees(&m.room, p)).cloned().collect();
            let asked = match owner {
                None => Asked::OpenWeb,
                Some(o) if o == m.requester => Asked::Themselves,
                Some(_) => Asked::Standing,
            };
            Some((w.name.clone(), room.name.clone(), readers, asked))
        });
        let Some((ws_name, room_name, readers, asked)) = place else {
            return hold("its room could not be found");
        };
        let mut input = json!({ "workspace": ws_name, "room": room_name, "text": summary });
        let mut standing = asked.standing();
        if !edits.is_empty() {
            input["edits"] = json!(edits.iter().map(|(file, changes)| json!({ "file": file, "changes": changes })).collect::<Vec<_>>());
            let files: Vec<&str> = edits.iter().map(|(f, _)| f.as_str()).collect();
            standing.push_str(&format!("; with it go edits to co-edited files of the shared project ({}), which its members read and may send on", files.join(", ")));
        }
        let send = Send {
            run,
            call_id: format!("release:{lane}"),
            tool: RELEASE_TOOL,
            app: "team",
            audience: Audience::Internal,
            input,
            readers: ("room_members", readers),
            text: summary,
            content: edits.iter().map(|(file, changes)| (format!("edit of {file}"), changes.clone())).collect(),
            to: "the room",
            covered: matches!(asked, Asked::Standing),
            within: Some(workspace),
            standing,
        };
        self.judged(send).await
    }

    /// Every check of a send that would leave by itself, in order, each of
    /// which may only hold: the scrubber, the run's taint, the outgoing
    /// classifier and Auto Review (outside agents' replies come here too).
    pub(crate) async fn checked_send(&self, send: Send<'_>) -> Gate {
        if let Some(g) = first_holds(send.text, send.run, send.to, send.within) {
            return g;
        }
        self.judged(send).await
    }

    /// The outgoing classifier, then Auto Review judging the send.
    async fn judged(&self, send: Send<'_>) -> Gate {
        let Send { run, call_id, tool, app, audience, input, readers, text, content, covered, standing, .. } = send;
        let action = Action { run: run.id.clone(), call_id: call_id.clone(), step_id: String::new(), tool: tool.into(), input, issuer: Issuer::Model };
        let content = std::iter::once(("text".to_string(), text.to_string())).chain(content).map(|(k, v)| (k, Value::String(v))).collect();
        let shown = Shown { recipients: [(readers.0.to_string(), readers.1)].into(), content, bound: Default::default() };
        let hash = shown.hash(tool, &action.input);
        match self.outgoing_finding(&action, run).await {
            None => return hold("the outgoing classifier could not be reached"),
            Some(Some(finding)) => return hold(format!("the outgoing classifier found something sensitive: {finding}")),
            Some(None) => {}
        }
        let facts =
            Facts { name: tool.into(), app: app.into(), risk: Risk::ReversibleWrite, label: Label::PRINCIPAL, audience, connector: None, remote: tool.into(), reaches_self: false };
        let verdict = self.auto_review_found(&action, &facts, run, Some(&shown), &hash, &standing, Some(None)).await;
        match verdict {
            // As for any boundary action (`asks::settle`): an allow acts
            // alone where the owner's policy delegates, or where the person
            // set up what covers it (their standing release).
            Ok(Verdict::Allow(_)) if self.auto_review_acts() || covered => Gate::Post,
            Ok(Verdict::Allow(_)) => hold("the owner's policy does not let Auto Review decide this alone"),
            Ok(Verdict::Deny(why) | Verdict::Ask(why) | Verdict::Unavailable(why)) => hold(format!("Auto Review held it: {why}")),
            Err(e) => {
                tracing::warn!("Auto Review of {call_id}: {e:#}");
                hold("Auto Review could not be reached")
            }
        }
    }
}
