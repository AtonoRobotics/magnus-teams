//! Sending a staged item: the staged action itself, performed through the
//! gate as an approved action is (decisions/2026-11-23-staged-sends-are-performed.md).
//!
//! Where it goes is read again first. Only when that is exactly what was
//! staged does the person's Send become a once approval for that very
//! action, and the action runs through `Kernel::execute`: its idempotency
//! key, intent, receipt and read-back are the boundary's own. A changed
//! draft (one added Bcc) is not sent: it is staged again, showing where it
//! would go now. The key comes from the staged item's id, so the action is
//! performed once however often Send is pressed.
use super::{StageMeta, StagedItem};
use crate::boundary::{idempotency_key, shown_hash, Action, Issuer};
use crate::events::Ev;
use crate::kernel::{new_id, Kernel};
use anyhow::{anyhow, Result};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

/// Why a staged item is staged again instead of sent.
const CHANGED: &str = "where it goes changed after it was staged";

/// What Send came to.
pub(crate) enum Sent {
    /// The action ran and its result was recorded as done.
    Done,
    /// Where it goes changed: not sent, staged again as this item.
    Changed(String),
    /// Not done, and why; the item stays open.
    NotDone(String),
}

/// Staged items being resolved now: one resolution at a time per item.
static RESOLVING: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// The hold on one item's resolution, let go when dropped.
pub(crate) struct Claim(String);

impl Claim {
    /// The hold on `id`, unless another resolution has it.
    pub(crate) fn take(id: &str) -> Option<Claim> {
        let mut held = RESOLVING.lock().unwrap_or_else(|e| e.into_inner());
        held.insert(id.to_string()).then(|| Claim(id.to_string()))
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        RESOLVING.lock().unwrap_or_else(|e| e.into_inner()).remove(&self.0);
    }
}

impl Kernel {
    /// Perform `item` for its person, who sent it (`by`).
    pub(crate) async fn send_staged(self: &Arc<Self>, item: &StagedItem, by: &str) -> Result<Sent> {
        if by != format!("person:{}", item.person) {
            return Err(anyhow!("only the person a staged item is for sends it"));
        }
        let run = self.read(|s| s.runs.get(&item.run).cloned()).ok_or_else(|| anyhow!("unknown run {}", item.run))?;
        let person = self.person(&run.person).ok_or_else(|| anyhow!("unknown person {}", run.person))?;
        let Some(f) = crate::boundary::facts(self, &item.tool) else {
            return Ok(Sent::NotDone(format!("{} is not a tool this server can use now", item.tool)));
        };
        // The item's id is the call and the step: one key, whatever happens.
        let a = Action { run: item.run.clone(), call_id: item.id.clone(), step_id: item.id.clone(), tool: item.tool.clone(), input: item.input.clone(), issuer: Issuer::Model };
        let shown = match self.read_destinations(&a, &f, &run, &person).await {
            Ok(s) => s,
            Err(why) => return Ok(Sent::NotDone(why)),
        };
        let hash = shown_hash(&a, shown.as_ref());
        let dests = self.destinations_of(&a, &f, shown.as_ref());
        if item.hash.as_deref() != Some(hash.as_str()) {
            let meta = StageMeta { kind: item.kind.clone(), dests: &dests, reason: CHANGED };
            let again = self.stage_item(&a, &run, &f, shown.as_ref(), meta).await?;
            return Ok(Sent::Changed(again));
        }
        // The person's Send is their approval of exactly this action, once.
        let approval = Ev::Approval {
            id: new_id("appr"),
            item: item.id.clone(),
            person: item.person.clone(),
            task: item.task.clone(),
            tool: item.tool.clone(),
            input_hash: Some(hash),
            scope: "once".into(),
            until: None,
            reason: Some(crate::asks::NEW_DESTINATION.into()),
            risk: Some(f.risk.as_str().into()),
            destinations: Some(dests),
        };
        self.emit(by, approval).await?;
        let key = idempotency_key(&a.run, &a.step_id, &a.tool);
        let verdict = Box::pin(self.execute(a)).await?;
        let result = self.read(|s| s.runs.get(&item.run).and_then(|r| r.intents.get(&key)).and_then(|i| i.result.clone()));
        match (result, verdict) {
            (Some(r), _) if !r.is_error && r.status == "done" => Ok(Sent::Done),
            (Some(r), _) => Ok(Sent::NotDone(r.text)),
            (None, crate::boundary::Verdict::Done { text, .. }) => Ok(Sent::NotDone(text)),
            (None, crate::boundary::Verdict::Suspended) => Ok(Sent::NotDone("it waits for the person before it can go".into())),
        }
    }
}
