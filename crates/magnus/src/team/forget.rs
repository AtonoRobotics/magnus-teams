//! What erasing a person means for the team ledger.
use super::chat::Chat;
use super::owner::person_of;
use super::{Change, Ledger};
use crate::events::Ev;
use crate::forget::{Erased, Forget};
use crate::kernel::Kernel;

/// They leave every workspace as a removal does, with what was held back
/// for them; their read marks, settings and reactions go, as do their
/// whispers, their requests' runs and notes, and their Team Admin role.
/// Their messages in every channel are deleted for everyone, as a sender's
/// own delete is: the text goes, the place stays so replies keep their
/// thread without naming them, and others' messages keep their own words (owner, 2026-11-13:
/// "yes", their data is deleted; decisions/2026-11-11-deleting-and-archiving-messages.md).
/// A project or workspace they owned was handed on or deleted before
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
            for m in w.channels.values_mut().flat_map(|ch| ch.messages.iter_mut()).filter(|m| who.is(person_of(&m.sender))) {
                m.text.clear();
                m.sender.clear();
                c.unsent.insert(m.id.clone());
                c.edited.remove(&m.id);
                c.pins.remove(&m.id);
                c.reactions.remove(&m.id);
                self.seqs.remove(&m.id);
            }
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

impl Ledger {
    /// Their messages not yet deleted: the workspace, the channel and the message.
    fn messages_of(&self, person: &str) -> Vec<(String, String, super::Message)> {
        let mut out = vec![];
        for w in self.workspaces.values() {
            for (id, ch) in &w.channels {
                let theirs = ch.messages.iter().filter(|m| person_of(&m.sender) == person && !w.chat.unsent.contains(&m.id));
                out.extend(theirs.map(|m| (w.id.clone(), id.clone(), m.clone())));
            }
        }
        out
    }
}

impl Kernel {
    /// Erasing `person`: each of their messages is deleted for everyone, in
    /// one write, as its sender would delete it. The events that held their
    /// text are returned, for the erasure to take out of the log with the
    /// rest of their content.
    pub(crate) async fn unsend_all_of(&self, actor: &str, person: &str) -> anyhow::Result<Vec<i64>> {
        let (theirs, seqs) = self.read(|s| {
            let theirs = s.team.messages_of(person);
            let seqs: Vec<i64> = theirs.iter().flat_map(|(_, _, m)| s.team.message_seqs(&m.id).to_vec()).collect();
            (theirs, seqs)
        });
        let evs: Vec<(&str, Ev)> = theirs
            .into_iter()
            .map(|(workspace, channel, m)| {
                // Its place names no one: the erasure forgets who sent it.
                let gone = Chat::Unsent { channel, id: m.id, sender: String::new(), thread: m.thread, at: m.at };
                (actor, Ev::Team(Change::Chat { workspace, chat: gone }))
            })
            .collect();
        if !evs.is_empty() {
            self.emit_all(evs).await?;
        }
        Ok(seqs)
    }
}
