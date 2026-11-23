//! A mission lane's life after dispatch: the one door every lane event is
//! written through (it asks the ledger's table, `LaneStatus::next`, under
//! one lock), the watcher that follows a lane's worker, and the pass at
//! boot that follows again the lanes a restart left working.

use super::missions::LaneStep;
use crate::compartment::query_gate::scrub_check;
use crate::events::{Ev, TeamMissionEvent};
use crate::kernel::Kernel;
use crate::state::Status;
use anyhow::Result;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Poll interval for the detached lane completion watcher (runs in its own task,
/// never blocking the Leader Agent's tool call).
const LANE_WATCH_POLL_MS: Duration = Duration::from_millis(250);
/// Maximum wall-clock time a detached lane watcher waits before marking the lane failed.
const LANE_WATCH_MAX_SECS: Duration = Duration::from_secs(15 * 60);

/// Who an open-web lane's release was credited to before 2026-11-22: no
/// one, so erasing the person who asked for it missed what it posted.
pub(crate) const OPEN_WEB_CREDIT: &str = "open_web";

/// Content rows an older release gave to `open_web` are the requester's of
/// the mission they answered: the release itself, and the room message it
/// posted (the same text, in the same workspace, after it). Run after
/// `log::CONTENT_OWNERS`, which gives such rows to `open_web` again.
pub const OPEN_WEB_OWNERS: &str = "UPDATE public.content c SET person = s.body->>'requester'
    FROM public.events e, public.events s
    WHERE c.seq = e.seq AND c.person = 'open_web' AND e.kind = 'team_mission' AND e.body->>'change' = 'lane_released'
      AND s.kind = 'team_mission' AND s.body->>'change' = 'started' AND s.body->>'mission' = e.body->>'mission';
UPDATE public.content c SET person = o.person FROM (
    SELECT m.seq, (SELECT s.body->>'requester' FROM public.events r JOIN public.content rc ON rc.seq = r.seq
            JOIN public.events s ON s.kind = 'team_mission' AND s.body->>'change' = 'started' AND s.body->>'mission' = r.body->>'mission'
        WHERE r.kind = 'team_mission' AND r.body->>'change' = 'lane_released' AND r.body->>'released_by' = 'open_web'
            AND r.body->>'workspace' = m.body->>'workspace' AND r.seq < m.seq AND rc.body->>'contribution' = mc.body->'message'->>'text'
        ORDER BY r.seq DESC LIMIT 1) AS person
    FROM public.events m JOIN public.content mc ON mc.seq = m.seq
    WHERE m.kind = 'team' AND m.body->>'change' = 'message' AND mc.person = 'open_web' AND mc.body->'message'->>'sender' = 'agent:open_web'
) o WHERE c.seq = o.seq AND o.person IS NOT NULL;";

/// Held from a lane event's check to its write, for every lane: the owner's
/// release and the watcher's can then never both pass.
static LANE_MOVES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The step a lane event is, when it is one. A release is either step: the
/// caller says which.
fn fits(step: LaneStep, ev: &TeamMissionEvent) -> Option<(&str, &str)> {
    match (step, ev) {
        (LaneStep::Draft, TeamMissionEvent::LaneDraftReady { mission, lane, .. })
        | (LaneStep::AutoRelease | LaneStep::OwnerRelease, TeamMissionEvent::LaneReleased { mission, lane, .. })
        | (LaneStep::Fail, TeamMissionEvent::LaneFailed { mission, lane, .. }) => Some((mission, lane)),
        _ => None,
    }
}

impl Kernel {
    /// Write a lane event when the ledger's table lets the lane take `step`
    /// now. `Ok(false)` when it cannot: it was released or failed meanwhile,
    /// or it is still working and the owner tried to release it. Only on
    /// `Ok(true)` does the caller post to the room or redo the deliverable.
    pub(crate) async fn move_lane(&self, actor: &str, step: LaneStep, ev: TeamMissionEvent) -> Result<bool> {
        let Some((mission, lane)) = fits(step, &ev) else {
            anyhow::bail!("{step:?} is not what this event does");
        };
        let _held = LANE_MOVES.lock().await;
        if !self.read(|s| s.missions.lane_can(mission, lane, step)) {
            return Ok(false);
        }
        self.emit(actor, Ev::TeamMission(ev)).await?;
        Ok(true)
    }

    /// After a restart, follow again every lane still working: a watcher
    /// lived only in the process that dispatched it. A worker still running
    /// is followed to its end. A lane whose worker ended while the server
    /// was down, or is gone, or that had none, fails: its result is not
    /// posted into the room late, perhaps long after it was asked for (a
    /// lane is held until its owner asks again).
    pub(crate) async fn resume_lanes(self: &Arc<Self>) {
        for w in self.read(|s| s.missions.working_lanes()) {
            let live = w.child_run.as_deref().map(|run| self.read(|s| s.runs.get(run).map(|r| !r.status.terminal())));
            let reason = match (w.child_run, live) {
                (Some(run), Some(Some(true))) => {
                    self.spawn_lane_watcher(w.workspace, w.mission, w.lane, w.owner, run, w.auto_release, w.thread);
                    continue;
                }
                (Some(_), Some(Some(_))) => "its worker ended while the server was down",
                (Some(_), _) => "worker run disappeared",
                (None, _) => "the lane had no worker to follow",
            };
            let failed = TeamMissionEvent::LaneFailed { workspace: w.workspace, mission: w.mission, lane: w.lane, reason: reason.into() };
            if let Err(e) = self.move_lane("kernel", LaneStep::Fail, failed).await {
                tracing::warn!("could not end a lane the restart left working: {e:#}");
            }
        }
    }

    /// Detached async watcher that routes a finished lane either straight to
    /// `LaneReleased` (when `auto_release` AND `scrub_check` passes) or into
    /// the owner's private Release Airlock (`LaneDraftReady`).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn spawn_lane_watcher(
        self: &Arc<Self>,
        workspace: String,
        mission: String,
        lane: String,
        owner: Option<String>,
        child_run: String,
        auto_release: bool,
        thread: Option<String>,
    ) {
        let k = self.clone();
        tokio::spawn(async move {
            let started = Instant::now();
            let failed = |reason: &str| TeamMissionEvent::LaneFailed { workspace: workspace.clone(), mission: mission.clone(), lane: lane.clone(), reason: reason.into() };
            loop {
                // A lane that left work (a steer's draft) is no one's to follow.
                if !k.read(|s| s.missions.lane_can(&mission, &lane, LaneStep::Fail)) {
                    return;
                }
                let Some(r) = k.read(|s| s.runs.get(&child_run).cloned()) else {
                    let _ = k.move_lane("kernel", LaneStep::Fail, failed("worker run disappeared")).await;
                    return;
                };
                if r.status.terminal() {
                    // Only a finished run is anything to share: a stopped
                    // one (its person's Stop, their removal) shares nothing.
                    if r.status != Status::Completed {
                        let reason = if r.status == Status::Cancelled { "worker run was stopped" } else { "worker run failed" };
                        let _ = k.move_lane("kernel", LaneStep::Fail, failed(reason)).await;
                        return;
                    }
                    let deliverable = r.outcome_view.as_ref().map(crate::sub_agents::render_deliverable).unwrap_or_default();
                    let raw_summary = r.summary.clone().or_else(|| r.replies.last().map(|m| m.text().to_string())).unwrap_or_default();
                    let summary = if !deliverable.is_empty() {
                        if !raw_summary.trim().is_empty() && !deliverable.contains(raw_summary.trim()) {
                            format!("{deliverable}\n\nSummary: {}", raw_summary.trim())
                        } else {
                            deliverable
                        }
                    } else if !raw_summary.trim().is_empty() {
                        raw_summary
                    } else {
                        "(finished without a text summary)".to_string()
                    };
                    let scrub_flagged = scrub_check(&summary).is_some();
                    if auto_release && !scrub_flagged {
                        let Some(credit) = k.read(|s| s.missions.release_credit(&mission, &lane)) else {
                            return;
                        };
                        let released = TeamMissionEvent::LaneReleased {
                            workspace: workspace.clone(),
                            mission: mission.clone(),
                            lane: lane.clone(),
                            contribution: summary.clone(),
                            released_by: credit.clone(),
                        };
                        if k.move_lane("kernel", LaneStep::AutoRelease, released).await.unwrap_or(false) {
                            k.post_released_contribution(&workspace, &mission, &credit, &summary, thread).await;
                            k.refresh_mission_deliverable(&workspace, &mission).await;
                        }
                    } else if let Some(owner) = owner {
                        let draft =
                            TeamMissionEvent::LaneDraftReady { workspace: workspace.clone(), mission: mission.clone(), lane: lane.clone(), owner, draft: summary, scrub_flagged };
                        let _ = k.move_lane("kernel", LaneStep::Draft, draft).await;
                    } else {
                        // No one's Airlock to hold it in: it left the lane
                        // working for ever.
                        let _ = k.move_lane("kernel", LaneStep::Fail, failed("the result held something that cannot be shared")).await;
                    }
                    return;
                }
                if started.elapsed() > LANE_WATCH_MAX_SECS {
                    let _ = k.move_lane("kernel", LaneStep::Fail, failed("lane timed out")).await;
                    return;
                }
                tokio::time::sleep(LANE_WATCH_POLL_MS).await;
            }
        });
    }
}
