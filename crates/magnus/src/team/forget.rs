//! What erasing a person means for the team ledger.
use super::Ledger;
use crate::forget::{Erased, Forget};

/// They leave every workspace as a removal does, with what was held back
/// for them; their read marks, settings and reactions go, as do their
/// whispers, their requests' runs and notes, and their Team Admin role.
/// Their messages in shared channels stay: a channel is its workspace's
/// record, the others keep their copy, and only a sender deletes a
/// message (decisions/2026-11-11-deleting-and-archiving-messages.md). A
/// project or workspace they owned was handed on or deleted before
/// (decisions/2026-11-11-admin-sections.md, Removing a person).
impl Forget for Ledger {
    fn forget(&mut self, who: &Erased) {
        let p = &who.person;
        for w in self.workspaces.values_mut() {
            w.members.remove(p);
            w.held.remove(p);
            for d in w.projects.values_mut() {
                d.members.retain(|m| m != p);
                d.curators.retain(|c| c != p);
            }
            let c = &mut w.chat;
            c.notify.remove(p);
            c.read.remove(p);
            c.archived.remove(p);
            c.cleared.remove(p);
            for by in c.reactions.values_mut() {
                for people in by.values_mut() {
                    people.remove(p);
                }
                by.retain(|_, people| !people.is_empty());
            }
            c.reactions.retain(|_, by| !by.is_empty());
        }
        self.whispers.retain(|task, w| !who.is(&w.owner) && !who.tasks.contains(task));
        self.runs.retain(|task, r| !who.is(&r.requester) && !who.tasks.contains(task));
        self.notes.retain(|task, _| !who.tasks.contains(task));
        self.admins.forget(p);
    }
}
