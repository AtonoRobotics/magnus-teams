//! A team-agent run as releases before 2026-11-22 recorded one, for the
//! tests of what still reads one: no path starts one now
//! (2026-11-22-retire-team-agent-runs), and the log keeps those it holds.
use super::TeamRun;
use crate::kernel::{Kernel, NewRun};
use anyhow::Result;
use std::sync::Arc;

impl Kernel {
    /// `person`'s request in `channel`, recorded as an older release's team run.
    pub(super) async fn recorded_team_run(self: &Arc<Self>, workspace: &str, channel: &str, person: &str, text: &str) -> Result<(String, String)> {
        let team = TeamRun {
            workspace: workspace.into(),
            channel: channel.into(),
            thread: None,
            task: String::new(),
            requester: person.into(),
            history: String::new(),
            sub: false,
            outside: false,
        };
        self.create_run(&format!("person:{person}"), person, text.to_string(), vec![], NewRun { team: Some(team), ..Default::default() }).await
    }
}
